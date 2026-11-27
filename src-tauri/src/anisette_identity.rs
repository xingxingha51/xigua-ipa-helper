//! The device identity we present to Apple, and keeping the stored anisette
//! state in step with it.
//!
//! isideload sends Apple whatever the anisette server reports from
//! `/v3/client_info`, as `X-Mme-Client-Info` and `User-Agent`, for
//! provisioning and sign-in alike. Apple has been refusing those identities one
//! at a time:
//!
//! - Sept 2026: `com.apple.dt.Xcode` → HTTP 503 at sign-in.
//! - 2026-11-20: anything spelled `<macOS;…>` → HTTP 503 at sign-in. That is
//!   what every Android-ADI server reports (`ani.sidestore.io`, `.app`, …).
//! - The `<Mac OS X;10.12.6;…>` of the Mac-based servers gets past that, but
//!   many accounts are then refused with -22410, or -23782 ("A Mac with macOS
//!   version 10.13 or later is required"). See nab138/iloader#1205.
//!
//! A current macOS release in the old `Mac OS X;` spelling works, so we present
//! our own identity whatever the server says. A copy published at
//! [`PUBLISHED_URL`] takes precedence over the built-in one, so the next change
//! on Apple's side can be answered without a release.
//!
//! The stored anisette state (adi.pb) only works at the server that issued it,
//! under the identity it was provisioned with — Apple answers -22410 when that
//! identity changes. [`sync_state`] tags the state with both and clears it when
//! either changes, so the next sign-in provisions again instead of failing.

use std::sync::Arc;
use std::time::Duration;

use isideload::anisette::{AnisetteClientInfo, AnisetteData, AnisetteProvider};
use isideload::auth::grandslam::GrandSlam;
use rootcause::prelude::*;
use serde_json::Value;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;
use tokio::sync::{Mutex, OnceCell};
use tracing::{info, warn};

use crate::error::AppError;

/// macOS 27.2.1 (26B131), the build this was verified against on 2026-11-27.
///
/// Keep identical to the store app's `AnisetteIdentity.builtIn` and to the
/// published file. Accounts sent to the phone carry adi.pb provisioned under
/// this identity; a phone using a different one has to provision again, which
/// costs the user another 2FA prompt.
pub const BUILT_IN_CLIENT_INFO: &str =
    "<MacBookPro18,3> <Mac OS X;27.2.1;26B131> <com.apple.AuthKit/1 (com.apple.akd/1.0)>";
pub const BUILT_IN_USER_AGENT: &str = "akd/1.0 CFNetwork/3896.200.61 Darwin/27.2.0";

/// Where the identity override is published, next to the server list the
/// store app reads.
pub const PUBLISHED_URL: &str = "https://loveipa.com/anisette-servers.json";

/// Key in `data.json` recording which server and identity the stored anisette
/// state was provisioned for.
const STATE_TAG_KEY: &str = "anisetteStateTag";

static CURRENT: OnceCell<AnisetteClientInfo> = OnceCell::const_new();
static SYNC_LOCK: Mutex<()> = Mutex::const_new(());

pub fn built_in() -> AnisetteClientInfo {
    AnisetteClientInfo {
        client_info: BUILT_IN_CLIENT_INFO.to_string(),
        user_agent: BUILT_IN_USER_AGENT.to_string(),
    }
}

/// The identity for this run: the published one when it can be fetched and is
/// valid, the built-in one otherwise.
///
/// Settled once per run, so two sign-ins in one session never disagree — a
/// disagreement would clear the anisette state between them.
pub async fn current() -> AnisetteClientInfo {
    CURRENT
        .get_or_init(|| async {
            match fetch_published().await {
                Ok(Some(identity)) => {
                    info!("Using the published anisette identity");
                    identity
                }
                Ok(None) => built_in(),
                Err(e) => {
                    warn!("Couldn't fetch the published anisette identity, using the built-in one: {e}");
                    built_in()
                }
            }
        })
        .await
        .clone()
}

async fn fetch_published() -> Result<Option<AnisetteClientInfo>, reqwest::Error> {
    let body = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get(PUBLISHED_URL)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    Ok(parse_published(&body))
}

/// Reads the `identity` object of the published file. Anything missing or
/// malformed falls back to the built-in identity rather than reaching Apple.
fn parse_published(body: &[u8]) -> Option<AnisetteClientInfo> {
    let root: Value = serde_json::from_slice(body).ok()?;
    let identity = root.get("identity")?;
    let parsed = validated(
        identity.get("clientInfo")?.as_str()?,
        identity.get("userAgent")?.as_str()?,
    );
    if parsed.is_none() {
        warn!("Ignoring the published anisette identity: it doesn't look valid");
    }
    parsed
}

/// Accepts only values that can go into an HTTP header and a client info shaped
/// `<model> <os;version;build> <framework (app)>`.
///
/// Says nothing about *which* OS spelling or version is right: that is exactly
/// what Apple keeps changing, and the published copy exists to follow it.
pub fn validated(client_info: &str, user_agent: &str) -> Option<AnisetteClientInfo> {
    let header_safe =
        |s: &str| !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| (0x20..0x7f).contains(&b));

    if !header_safe(client_info) || !header_safe(user_agent) || !has_client_info_shape(client_info)
    {
        return None;
    }

    Some(AnisetteClientInfo {
        client_info: client_info.to_string(),
        user_agent: user_agent.to_string(),
    })
}

/// Exactly three `<…>` groups separated by single spaces, none of them empty
/// or nested.
fn has_client_info_shape(s: &str) -> bool {
    let mut rest = s;
    for group in 0..3 {
        let Some(after_open) = rest.strip_prefix('<') else {
            return false;
        };
        let Some(close) = after_open.find('>') else {
            return false;
        };
        let inner = &after_open[..close];
        if inner.is_empty() || inner.contains('<') {
            return false;
        }
        rest = &after_open[close + 1..];
        if group < 2 {
            let Some(after_space) = rest.strip_prefix(' ') else {
                return false;
            };
            rest = after_space;
        }
    }
    rest.is_empty()
}

/// `ani.example.com`, `https://ani.example.com/` → `https://ani.example.com`.
///
/// The settings may hold a bare host, while the store app and the state tag
/// want a URL; normalizing both sides keeps them comparable.
pub fn normalize_server(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

fn state_tag(server: &str, identity: &AnisetteClientInfo) -> String {
    format!("{}\n{}", normalize_server(server), identity.client_info)
}

/// Clears the stored anisette state when it was provisioned for another server
/// or under another identity, then records the current pair.
///
/// Returns whether a state was actually cleared, i.e. whether the next sign-in
/// will ask for a 2FA code again. A first run of this version clears whatever
/// older versions provisioned: that was done under the server's own identity,
/// which Apple no longer accepts.
pub async fn sync_state(
    app: &AppHandle,
    server: &str,
    identity: &AnisetteClientInfo,
) -> Result<bool, AppError> {
    let _guard = SYNC_LOCK.lock().await;

    let store = app
        .store("data.json")
        .map_err(|e| AppError::Misc(format!("Failed to get store: {e:?}")))?;
    let wanted = state_tag(server, identity);
    if store.get(STATE_TAG_KEY).as_ref().and_then(Value::as_str) == Some(wanted.as_str()) {
        return Ok(false);
    }

    let removed = crate::account::reset_anisette_state(app.clone())?;
    store.set(STATE_TAG_KEY, Value::String(wanted));
    store
        .save()
        .map_err(|e| AppError::Misc(format!("Failed to save store: {e:?}")))?;

    if removed {
        info!("Cleared anisette state provisioned for another server or identity");
    }
    Ok(removed)
}

/// Lets the UI warn about the coming 2FA prompt at startup rather than in the
/// middle of a sign-in. Sign-ins run the same check themselves.
#[tauri::command]
pub async fn prepare_anisette(app: AppHandle, anisette_server: String) -> Result<bool, AppError> {
    let identity = current().await;
    sync_state(&app, &anisette_server, &identity).await
}

/// Presents `identity` to Apple in place of the anisette server's own.
///
/// isideload builds its GrandSlam client from `get_client_info()`, and every
/// GrandSlam request carries those headers — provisioning included — so this
/// one method covers both. Everything else goes to the wrapped provider.
pub struct WithIdentity<P> {
    inner: P,
    identity: AnisetteClientInfo,
}

impl<P> WithIdentity<P> {
    pub fn new(inner: P, identity: AnisetteClientInfo) -> Self {
        Self { inner, identity }
    }
}

#[async_trait::async_trait]
impl<P: AnisetteProvider + Send + Sync> AnisetteProvider for WithIdentity<P> {
    async fn get_anisette_data(&self) -> Result<AnisetteData, Report> {
        self.inner.get_anisette_data().await
    }

    async fn get_client_info(&mut self) -> Result<AnisetteClientInfo, Report> {
        Ok(self.identity.clone())
    }

    async fn provision(&mut self, gs: Arc<GrandSlam>) -> Result<(), Report> {
        self.inner.provision(gs).await
    }

    fn needs_provisioning(&self) -> Result<bool, Report> {
        self.inner.needs_provisioning()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_identity_passes_validation() {
        assert!(validated(BUILT_IN_CLIENT_INFO, BUILT_IN_USER_AGENT).is_some());
    }

    #[test]
    fn built_in_identity_uses_the_spelling_apple_accepts() {
        // The edge has answered 503 to `<macOS;` since 2026-11-20, and to the
        // Xcode identity since September.
        assert!(BUILT_IN_CLIENT_INFO.contains("<Mac OS X;"));
        assert!(!BUILT_IN_CLIENT_INFO.contains("macOS;"));
        assert!(BUILT_IN_CLIENT_INFO.contains("(com.apple.akd/1.0)"));
    }

    #[test]
    fn validation_rejects_malformed_values() {
        let ua = BUILT_IN_USER_AGENT;
        for bad in [
            "",
            "<a> <b>",
            "<a> <b> <c> <d>",
            "<a>  <b> <c>",
            "<a> <b> <c> ",
            "<> <b> <c>",
            "<a<b> <c> <d>",
            "<a> <b> <c>\r\nX-Injected: 1",
        ] {
            assert!(validated(bad, ua).is_none(), "accepted {bad:?}");
        }
        assert!(validated(BUILT_IN_CLIENT_INFO, "").is_none());
        assert!(validated(BUILT_IN_CLIENT_INFO, "akd\n").is_none());
        assert!(validated(&format!("<{}> <b> <c>", "a".repeat(300)), ua).is_none());
    }

    #[test]
    fn published_identity_is_used_only_when_valid() {
        let valid = br#"{"servers":[],"identity":{"clientInfo":"<Mac1,1> <Mac OS X;28.0;27A1> <com.apple.AuthKit/1 (com.apple.akd/1.0)>","userAgent":"akd/1.0 CFNetwork/4000 Darwin/28.0.0"}}"#;
        let parsed = parse_published(valid).expect("valid identity ignored");
        assert!(parsed.client_info.contains("28.0;27A1"));

        assert!(parse_published(br#"{"servers":[]}"#).is_none());
        assert!(parse_published(br#"{"identity":{"clientInfo":"nope","userAgent":"x"}}"#).is_none());
        assert!(parse_published(br#"{"identity":"<a> <b> <c>"}"#).is_none());
        assert!(parse_published(b"not json").is_none());
    }

    #[test]
    fn state_tag_ignores_how_the_server_is_written() {
        let identity = built_in();
        assert_eq!(
            state_tag("new.ani.sidestore.io", &identity),
            state_tag("https://new.ani.sidestore.io/", &identity)
        );
        assert_ne!(
            state_tag("new.ani.sidestore.io", &identity),
            state_tag("ani.stikstore.app", &identity)
        );
        let other = validated(
            "<MacBookPro13,3> <Mac OS X;10.12.6;16G29> <com.apple.AuthKit/1 (com.apple.akd/1.0)>",
            BUILT_IN_USER_AGENT,
        )
        .unwrap();
        assert_ne!(
            state_tag("new.ani.sidestore.io", &identity),
            state_tag("new.ani.sidestore.io", &other)
        );
    }

    /// Provisions a fresh device through `WithIdentity` against a live server
    /// and asks it for anisette headers — the path every sign-in takes, minus
    /// the Apple ID. Talks to Apple and the server, so it only runs on request:
    /// `cargo test -- --ignored provisions_with_our_identity`.
    #[tokio::test]
    #[ignore]
    async fn provisions_with_our_identity() {
        use isideload::anisette::AnisetteDataGenerator;
        use isideload::anisette::remote_v3::RemoteV3AnisetteProvider;
        use isideload::util::fs_storage::FsStorage;
        use tokio::sync::RwLock;

        let server =
            std::env::var("ANISETTE_TEST_SERVER").unwrap_or("https://new.ani.sidestore.io".into());
        let dir = std::env::temp_dir().join(format!("xigua-anisette-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let identity = built_in();
        let provider = RemoteV3AnisetteProvider::default()
            .unwrap()
            .set_serial_number("0".into())
            .set_storage(Box::new(FsStorage::new(dir.clone())))
            .set_url(&server);
        let mut generator = AnisetteDataGenerator::new(Arc::new(RwLock::new(WithIdentity::new(
            provider,
            identity.clone(),
        ))));

        // What AppleAccount does: GrandSlam gets its headers from the provider.
        let client_info = generator.get_client_info().await.unwrap();
        assert_eq!(client_info.client_info, identity.client_info);
        let gs = Arc::new(GrandSlam::new(client_info, false).await.unwrap());

        let result = generator.get_anisette_data(gs).await;
        std::fs::remove_dir_all(&dir).ok();
        let data = result.unwrap_or_else(|e| panic!("provisioning at {server} failed: {e:?}"));
        assert!(data.get_headers().contains_key("X-Apple-I-MD"));
    }

    /// `cargo test -- --ignored published_file_is_valid`
    #[tokio::test]
    #[ignore]
    async fn published_file_is_valid() {
        let published = fetch_published().await.expect("fetch failed");
        let identity = published.expect("the published file has no valid identity");
        // The two built-in copies and the published one should agree; if they
        // don't, phones and the helper provision under different identities.
        assert_eq!(identity.client_info, BUILT_IN_CLIENT_INFO);
        assert_eq!(identity.user_agent, BUILT_IN_USER_AGENT);
    }
}
