use std::{path::PathBuf, sync::Mutex};

use crate::{
    device::{DeviceInfoMutex, get_provider, get_provider_from_connection, get_usbmuxd},
    error::AppError,
    operation::Operation,
    pairing::{get_sidestore_info, place_file},
};
use isideload::sideload::{application::SpecialApp, sideloader::Sideloader};
use tauri::{AppHandle, Manager, State, Window};

pub type SideloaderMutex = Mutex<Option<Sideloader>>;

pub struct SideloaderGuard<'a> {
    state: &'a SideloaderMutex,
    sideloader: Option<Sideloader>,
}

impl<'a> SideloaderGuard<'a> {
    pub fn take(state: &'a SideloaderMutex) -> Result<Self, AppError> {
        let mut guard = state.lock().unwrap();
        let sideloader = guard.take().ok_or(AppError::NotLoggedIn)?;
        Ok(Self {
            state,
            sideloader: Some(sideloader),
        })
    }

    pub fn get_mut(&mut self) -> &mut Sideloader {
        self.sideloader
            .as_mut()
            .expect("Sideloader should be present")
    }
}

impl Drop for SideloaderGuard<'_> {
    fn drop(&mut self) {
        let mut guard = self.state.lock().unwrap();
        *guard = self.sideloader.take();
    }
}

pub async fn sideload(
    device_state: State<'_, DeviceInfoMutex>,
    sideloader_state: State<'_, SideloaderMutex>,
    app_path: String,
) -> Result<Option<SpecialApp>, AppError> {
    let device = {
        let device_lock = device_state.lock().unwrap();
        match &*device_lock {
            Some(d) => d.clone(),
            None => return Err(AppError::NoDeviceSelected),
        }
    };

    let provider = get_provider(&device.info).await?;

    let mut sideloader = SideloaderGuard::take(&sideloader_state)?;

    let special = sideloader
        .get_mut()
        .install_app(&provider, app_path.into(), false)
        .await?;

    Ok(special)
}

#[tauri::command]
pub async fn sideload_operation(
    window: Window,
    device_state: State<'_, DeviceInfoMutex>,
    sideloader_state: State<'_, SideloaderMutex>,
    app_path: String,
) -> Result<(), AppError> {
    let op = Operation::new("sideload".to_string(), &window);
    op.start("install")?;
    op.fail_if_err(
        "install",
        sideload(device_state, sideloader_state, app_path).await,
    )?;
    op.complete("install")?;
    Ok(())
}

#[tauri::command]
pub async fn install_sidestore_operation(
    handle: AppHandle,
    window: Window,
    device_state: State<'_, DeviceInfoMutex>,
    sideloader_state: State<'_, SideloaderMutex>,
    #[allow(unused_variables)] nightly: bool,
    live_container: bool,
    sync_account: bool,
    email: Option<String>,
    anisette_server: Option<String>,
) -> Result<(), AppError> {
    let op = Operation::new("install_sidestore".to_string(), &window);
    op.start("download")?;
    // TODO: Cache & check version to avoid re-downloading
    //
    // Upstream branched here on nightly/live_container to install stock SideStore
    // or the LiveContainer+SideStore bundle. This build only ever installs 西瓜IPA助手 —
    // the UI hard-codes both flags to false, and installing stock SideStore would
    // hand the user an English app with no bundled source and none of the trimming,
    // i.e. none of the reasons this fork exists.
    // 先走自己的域名。国内直连 GitHub releases 经常极慢甚至断流,用户就卡在
    // 这一步不动 —— 这是个 24 MB 的包。loveipa.com 上的 /dl/ 是一层
    // Cloudflare 转发,边缘去取 GitHub,客户端只跟我们的域名打交道。
    //
    // GitHub 留作兜底:转发那层要是挂了或改了,不该把安装整个堵死。
    let filename = "XiguaStore.ipa";
    let urls = [
        "https://loveipa.com/dl/xigua-store/latest/XiguaStore.ipa",
        "https://github.com/xingxingha51/xigua-store/releases/latest/download/XiguaStore.ipa",
    ];

    let dest = handle
        .path()
        .temp_dir()
        .map_err(|e| AppError::Filesystem("Failed to get temp dir".into(), e.to_string()))?
        .join(filename);
    op.fail_if_err("download", download_from_first_available(&urls, &dest).await)?;
    op.move_on("download", "install")?;
    let device = {
        let device_guard = device_state.lock().unwrap();
        match &*device_guard {
            Some(d) => d.clone(),
            None => return op.fail("install", AppError::NoDeviceSelected),
        }
    };
    op.fail_if_err(
        "install",
        sideload(
            device_state,
            sideloader_state,
            dest.to_string_lossy().to_string(),
        )
        .await,
    )?;
    op.move_on("install", "pairing")?;
    let sidestore_info = op.fail_if_err(
        "pairing",
        get_sidestore_info(&device.info, live_container).await,
    )?;
    let store_bundle_id;
    if let Some(info) = sidestore_info {
        store_bundle_id = info.bundle_id.clone();
        let mut usbmuxd = op.fail_if_err("pairing", get_usbmuxd().await)?;

        let provider = op.fail_if_err(
            "pairing",
            get_provider_from_connection(&device.info, &mut usbmuxd).await,
        )?;

        op.fail_if_err(
            "pairing",
            place_file(device.pairing, &provider, info.bundle_id, info.path).await,
        )?;
    } else {
        return op.fail(
            "pairing",
            AppError::HouseArrest(
                "找不到已安装的西瓜IPA助手".into(),
                "设备没有报告西瓜IPA助手的 Bundle ID，安装可能未完成。请在手机上确认它已出现在桌面后重试。".into(),
            ),
        );
    }

    op.move_on("pairing", "account")?;

    // Optional last step. The app is already installed and paired by now, so a
    // failure here is reported on its own step rather than failing the install —
    // the user can still sign in on the device by hand.
    match (sync_account, email, anisette_server) {
        (true, Some(email), Some(anisette_server)) => {
            let payload = crate::account_export::prepare_account_payload(
                &handle,
                &window,
                &email,
                None,
                &anisette_server,
            )
            .await;

            match payload {
                Ok(json) => {
                    let provider = op.fail_if_err("account", get_provider(&device.info).await)?;
                    op.fail_if_err(
                        "account",
                        place_file(json, &provider, store_bundle_id, "Account.sideconf".into())
                            .await,
                    )?;
                    op.complete("account")?;
                }
                Err(e) => return op.fail("account", e),
            }
        }
        _ => op.complete("account")?,
    }

    Ok(())
}

/// 依次试,第一个成功的就用。
///
/// 只在**整个下载都失败**时才换下一个 —— 半路断流的重试留给 reqwest 自己,
/// 这里换源是为了应对「这个域名根本连不上」。报错带上最后一次的原因,不然
/// 用户只看到「下载失败」,不知道是网的问题还是地址挂了。
async fn download_from_first_available(urls: &[&str], dest: &PathBuf) -> Result<(), AppError> {
    let mut last = None;
    for (i, url) in urls.iter().enumerate() {
        match download(url, dest).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                eprintln!("下载失败({}/{}) {url}: {e:?}", i + 1, urls.len());
                last = Some(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| AppError::Download("没有可用的下载地址".into())))
}

pub async fn download(url: impl AsRef<str>, dest: &PathBuf) -> Result<(), AppError> {
    let response = reqwest::get(url.as_ref())
        .await
        .map_err(|e| AppError::Download(e.to_string()))?;
    if !response.status().is_success() {
        return Err(AppError::Download(format!(
            "Failed to download file: HTTP {}",
            response.status()
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Download(e.to_string()))?;
    tokio::fs::write(dest, &bytes).await.map_err(|e| {
        AppError::Filesystem("Failed to write downloaded file".into(), e.to_string())
    })?;

    Ok(())
}
