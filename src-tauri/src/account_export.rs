//! Exports the signed-in Apple account as an `Account.sideconf` file that
//! 西瓜IPA助手 (our SideStore fork) can import, so the user only signs in once.
//!
//! The store app reads `Documents/Account.sideconf` on launch, imports it into
//! the keychain, and deletes it. This module produces that file; for now the
//! user transfers it manually so the format can be verified end-to-end before
//! we wire up automatic delivery over AFC.
//!
//! ## The file contains the Apple ID password in cleartext
//!
//! That is forced by what the store app needs: refreshing apps re-fetches
//! provisioning profiles from Apple, which requires re-authenticating, so it
//! keeps the password rather than just the certificate. Treat the exported file
//! as equivalent to the password itself — it is written to a user-chosen path
//! and should be deleted after import.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use isideload::{
    dev::{
        developer_session::DeveloperSession,
        teams::{DeveloperTeam, TeamsApi},
    },
    sideload::{builder::MaxCertsBehavior, cert_identity::CertificateIdentity},
    util::storage::SideloadingStorage,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Window};
use tauri_plugin_dialog::DialogExt;

use crate::{error::AppError, secure_storage::create_sideloading_storage};

/// Must match `machine_name` used when signing, or `CertificateIdentity`
/// requests a second certificate instead of reusing the existing one — and
/// free accounts only get one.
pub const MACHINE_NAME: &str = "xigua-ipa-helper";

/// Mirrors isideload's `AnisetteState`, which lives in a private module and so
/// can't be named here. The on-disk form is a plist with these two keys; if
/// upstream renames them this deserialization fails loudly rather than
/// silently exporting an account with no anisette data.
///
/// Both fields must go through `plist::Data`. isideload writes them with
/// `serialize_bytes`, which lands in the plist as `<data>`, and a plain
/// `Vec<u8>` asks serde for a sequence — that mismatch fails the parse with
/// "invalid type: byte array, expected a sequence".
#[derive(Deserialize)]
struct StoredAnisetteState {
    #[serde(deserialize_with = "bin_deserialize")]
    keychain_identifier: Vec<u8>,
    #[serde(deserialize_with = "bin_deserialize_opt")]
    adi_pb: Option<Vec<u8>>,
}

fn bin_deserialize<'de, D>(d: D) -> Result<Vec<u8>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let data: plist::Data = Deserialize::deserialize(d)?;
    Ok(data.into())
}

fn bin_deserialize_opt<'de, D>(d: D) -> Result<Option<Vec<u8>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let data: Option<plist::Data> = Deserialize::deserialize(d)?;
    Ok(data.map(Into::into))
}

/// Shape of `Account.sideconf`, matching `ImportedAccount` in the store app.
///
/// Field names are load-bearing: the store app decodes this with Swift's
/// `Codable`, which matches keys literally. `cert` is `Data` on that side, and
/// Swift's default `JSONDecoder` expects base64 for `Data`.
#[derive(Serialize)]
pub struct ExportedAccount {
    email: String,
    password: String,
    cert: String,
    certpass: String,
    local_user: String,
    #[serde(rename = "adiPB")]
    adi_pb: String,
    /// adi.pb is provisioned against one specific anisette server's ADI
    /// instance. Replaying it at a different server yields anisette Apple
    /// rejects, so the store has to be told which one issued it — it otherwise
    /// picks the first reachable entry from a 10-server public list.
    #[serde(rename = "anisetteServer")]
    anisette_server: String,
}

/// Builds the account payload from a signed-in developer session.
///
/// `storage` must be the same instance used for signing — the private key and
/// anisette state are read back out of it.
pub async fn build_exported_account(
    email: &str,
    password: &str,
    dev_session: &mut DeveloperSession,
    storage: &dyn SideloadingStorage,
    anisette_server: &str,
) -> Result<ExportedAccount, AppError> {
    let teams = dev_session
        .list_teams()
        .await
        .map_err(|e| AppError::Misc(format!("Failed to list developer teams: {e:?}")))?;

    let team: DeveloperTeam = teams
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Misc("No developer team on this Apple ID".into()))?;

    // Reuses the existing certificate when one matches this machine name, so
    // exporting doesn't burn the single certificate a free account allows.
    //
    // `Error` rather than `Revoke`: revoking would invalidate whatever is
    // already signed on the user's devices, and exporting an account is not a
    // reason to do that to them.
    let identity = CertificateIdentity::retrieve(
        MACHINE_NAME,
        &email.to_lowercase(),
        dev_session,
        &team,
        storage,
        &MaxCertsBehavior::Error,
    )
    .await
    .map_err(|e| AppError::Misc(format!("Failed to obtain signing certificate: {e:?}")))?;

    // isideload documents the machine id as the password to use when the p12 is
    // destined for SideStore/AltStore.
    let machine_id = identity.machine_id.clone();
    let p12 = identity
        .as_p12(&machine_id)
        .await
        .map_err(|e| AppError::Misc(format!("Failed to export certificate as PKCS#12: {e:?}")))?;

    let raw_state = storage
        .retrieve_data("anisette_state")
        .map_err(|e| AppError::Misc(format!("Failed to read anisette state: {e:?}")))?
        .ok_or_else(|| {
            AppError::Misc("No anisette state stored — sign in before exporting".into())
        })?;

    let state: StoredAnisetteState = plist::from_bytes(&raw_state)
        .map_err(|e| AppError::Misc(format!("Failed to parse anisette state: {e:?}")))?;

    // identifier and adi.pb must come from the same provisioning: the blob is
    // issued against that specific identifier. Reading both from one state
    // keeps them paired.
    let adi_pb = state.adi_pb.ok_or_else(|| {
        AppError::Misc("Anisette state has no adi.pb — sign in first so it gets provisioned".into())
    })?;

    Ok(ExportedAccount {
        email: email.to_lowercase(),
        password: password.to_string(),
        cert: BASE64.encode(&p12),
        certpass: machine_id,
        local_user: BASE64.encode(state.keychain_identifier),
        adi_pb: BASE64.encode(adi_pb),
        anisette_server: normalize_anisette_url(anisette_server),
    })
}

/// The store stores server URLs with a scheme; the helper's setting may omit it.
fn normalize_anisette_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

impl ExportedAccount {
    pub fn to_json(&self) -> Result<Vec<u8>, AppError> {
        serde_json::to_vec(self)
            .map_err(|e| AppError::Misc(format!("Failed to encode account file: {e:?}")))
    }
}

/// Whether the password is already in the keychain, so the UI can skip asking.
#[tauri::command]
pub fn has_stored_password(email: String) -> bool {
    keyring::Entry::new(MACHINE_NAME, &email)
        .and_then(|entry| entry.get_password())
        .is_ok()
}

/// Signs in, builds the account payload, and writes it to a path the user picks.
///
/// Step one of the single-sign-in work: the file is produced here but carried
/// over manually, so the format can be proven against a real device before the
/// helper starts writing it to the phone by itself. A format mismatch would not
/// surface at import — the store app writes the fields to the keychain without
/// validating them — it would surface a week later as a failed refresh.
///
/// `password` is optional: when the user saved their credentials at sign-in we
/// read it back from the keychain instead of asking again. Prompting would
/// defeat the point of a feature whose whole purpose is one less login.
/// Signs in and builds the payload. Shared by both delivery paths so they can
/// never drift in what they put in the file.
async fn prepare_account_payload(
    app: &AppHandle,
    window: &Window,
    email: &str,
    password: Option<String>,
    anisette_server: &str,
) -> Result<Vec<u8>, AppError> {
    let storage = create_sideloading_storage(app)?;

    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => keyring::Entry::new(MACHINE_NAME, email)
            .and_then(|entry| entry.get_password())
            .map_err(|_| {
                AppError::Misc(
                    "没有保存的密码。请重新登录并勾选「保存凭据」，或手动输入密码。".into(),
                )
            })?,
    };

    let mut apple_account = crate::account::login_apple_account(
        app,
        window,
        email,
        &password,
        anisette_server.to_string(),
    )
    .await?;

    let mut dev_session = DeveloperSession::from_account(&mut apple_account)
        .await
        .map_err(|e| AppError::Misc(format!("Failed to create developer session: {e:?}")))?;

    let account = build_exported_account(
        email,
        &password,
        &mut dev_session,
        storage.as_ref(),
        anisette_server,
    )
    .await?;

    account.to_json()
}

/// Writes the account straight into the store app's Documents over AFC, so the
/// user never handles a file that contains their password in cleartext.
///
/// The store imports it on its next cold launch and deletes it immediately —
/// see `detectAndImportAccountFile`. Nothing is written to the desktop.
#[tauri::command]
pub async fn send_account_to_device(
    app: AppHandle,
    window: Window,
    device_state: tauri::State<'_, crate::device::DeviceInfoMutex>,
    email: String,
    password: Option<String>,
    anisette_server: String,
) -> Result<String, AppError> {
    let device = {
        let guard = device_state.lock().unwrap();
        match &*guard {
            Some(d) => d.clone(),
            None => return Err(AppError::NoDeviceSelected),
        }
    };

    // Locate the store first: signing in takes a while, and failing afterwards
    // because the app isn't installed would waste all of it.
    let info = crate::pairing::get_sidestore_info(&device.info, false)
        .await?
        .ok_or_else(|| {
            AppError::HouseArrest(
                "手机上没有找到西瓜IPA助手".into(),
                "请先用「一键安装」把它装到手机上，再发送账号。".into(),
            )
        })?;

    let json = prepare_account_payload(&app, &window, &email, password, &anisette_server).await?;

    let provider = crate::device::get_provider(&device.info).await?;
    crate::pairing::place_file(json, &provider, info.bundle_id, "Account.sideconf".into()).await?;

    Ok(info.name)
}

#[tauri::command]
pub async fn export_account_file(
    app: AppHandle,
    window: Window,
    email: String,
    password: Option<String>,
    anisette_server: String,
) -> Result<String, AppError> {
    let json = prepare_account_payload(&app, &window, &email, password, &anisette_server).await?;

    let save_path = app
        .dialog()
        .file()
        .add_filter("西瓜IPA助手账号文件", &["sideconf"])
        .set_file_name("Account.sideconf")
        .set_title("导出账号文件")
        .blocking_save_file();

    let Some(save_path) = save_path.and_then(|p| p.as_path().map(|p| p.to_path_buf())) else {
        return Err(AppError::Misc("已取消导出".into()));
    };

    tokio::fs::write(&save_path, &json)
        .await
        .map_err(|e| AppError::Misc(format!("Failed to write account file: {e:?}")))?;

    Ok(save_path.to_string_lossy().to_string())
}
