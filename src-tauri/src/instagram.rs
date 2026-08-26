// Instagram account connection: the official Meta Graph API OAuth flow
// ("Manage messaging & content on Instagram" use case), NOT any
// unofficial/private-API automation -- see the plan this was built from
// for why. Requires the user's own Meta Developer App (App ID + Secret,
// saved via `save_instagram_app_config`) with that use case added, their
// Instagram account converted to Business/Creator and linked to a
// Facebook Page, and (for personal use without a 2-4 week App Review)
// that Instagram account added as the app's Instagram Tester.
//
// OAuth uses the standard desktop-app loopback pattern: open the system
// browser to Meta's consent screen, catch the redirect on a short-lived
// local HTTP listener, exchange the code for tokens entirely server-side
// (well, app-side) via Graph API. No cloud intermediary, no embedded
// webview capturing credentials -- the user authenticates in their own
// real browser, where their existing Facebook session/2FA/password
// manager all just work normally.
//
// Publishing itself (Phase 3 in the plan) is a separate module -- this
// one only gets from "nothing connected" to "we have a Page access token
// + the linked Instagram Business Account's id, persisted locally."

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::util::cli_path;

const REDIRECT_URI: &str = "http://localhost:47829/instagram/callback";
const REDIRECT_PORT: &str = "47829";
const OAUTH_SCOPES: &str = "instagram_business_basic,instagram_business_content_publish,pages_show_list";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppConfig {
    app_id: String,
    app_secret: String,
}

fn app_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Couldn't resolve app data directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create app data directory: {e}"))?;
    Ok(dir.join("instagram-app-config.json"))
}

fn account_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Couldn't resolve app data directory: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create app data directory: {e}"))?;
    Ok(dir.join("instagram-account.json"))
}

fn load_app_config(app: &AppHandle) -> Result<AppConfig, String> {
    let path = app_config_path(app)?;
    let contents = std::fs::read_to_string(&path)
        .map_err(|_| "No Meta App ID/Secret saved yet -- enter them first.".to_string())?;
    serde_json::from_str(&contents).map_err(|e| format!("Couldn't read saved app config: {e}"))
}

#[tauri::command]
pub fn save_instagram_app_config(app: AppHandle, app_id: String, app_secret: String) -> Result<(), String> {
    let config = AppConfig { app_id, app_secret };
    let path = app_config_path(&app)?;
    let contents = serde_json::to_string(&config).map_err(|e| format!("Couldn't serialize app config: {e}"))?;
    std::fs::write(&path, contents).map_err(|e| format!("Couldn't write app config: {e}"))
}

#[tauri::command]
pub fn has_instagram_app_config(app: AppHandle) -> bool {
    app_config_path(&app).map(|p| p.exists()).unwrap_or(false)
}

/// Opens a URL in the system's default browser -- used by the in-app
/// setup guide's "Create Facebook Page" / "Open Meta Developer Apps"
/// buttons (`InstagramPanel.jsx`), same underlying plugin the OAuth
/// consent screen itself opens with.
#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| format!("Couldn't open browser: {e}"))
}

/// What's persisted after a successful connect -- everything
/// `voice_clone`... er, `instagram::publish_reel` (Phase 3) will need.
/// `page_access_token` (not the raw user token) is what Instagram's own
/// publish endpoints expect for an app using this permission model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgAccount {
    pub page_access_token: String,
    pub page_id: String,
    pub ig_user_id: String,
    pub username: String,
    /// Unix seconds. The long-lived user token this was derived from is
    /// valid ~60 days; re-authenticating (calling connect again) is the
    /// simplest refresh path until Phase 4 adds a real background
    /// refresh-before-expiry check.
    pub obtained_at: u64,
    pub expires_in_seconds: u64,
}

#[derive(Serialize)]
pub struct IgAccountSummary {
    pub username: String,
}

#[tauri::command]
pub fn get_connected_instagram_account(app: AppHandle) -> Option<IgAccountSummary> {
    let path = account_path(&app).ok()?;
    let contents = std::fs::read_to_string(path).ok()?;
    let account: IgAccount = serde_json::from_str(&contents).ok()?;
    Some(IgAccountSummary { username: account.username })
}

#[tauri::command]
pub fn disconnect_instagram_account(app: AppHandle) -> Result<(), String> {
    let path = account_path(&app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Couldn't remove saved account: {e}"))?;
    }
    Ok(())
}

fn now_unix_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Pulls one query-param value out of a request path like
/// "/instagram/callback?code=XYZ&state=abc" -- deliberately hand-rolled
/// rather than pulling in a URL-parsing crate for this one job; the
/// shape here is fully controlled (it's our own redirect_uri), not
/// arbitrary untrusted URLs.
fn query_param(path_and_query: &str, key: &str) -> Option<String> {
    let query = path_and_query.split_once('?')?.1;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            return Some(urlencoding::decode(v).ok()?.into_owned());
        }
    }
    None
}

/// Waits (blocking, so run inside `spawn_blocking`) for exactly one
/// request to the redirect URI, replies with a plain "you can close this
/// tab" page, and returns the `code` query param -- or an error message
/// from the `error_description` param if the user declined consent.
fn wait_for_oauth_redirect() -> Result<String, String> {
    let server = tiny_http::Server::http(format!("127.0.0.1:{REDIRECT_PORT}"))
        .map_err(|e| format!("Couldn't start local sign-in listener on port {REDIRECT_PORT}: {e}"))?;

    // Real browsers issue a handful of incidental requests (favicon.ico,
    // etc.) to a page before/after the real redirect -- keep listening
    // until we see one with an actual `code` or `error` param rather than
    // assuming the very first request is it.
    loop {
        let request = server.recv().map_err(|e| format!("Local sign-in listener error: {e}"))?;
        let url = request.url().to_string();

        if let Some(error) = query_param(&url, "error_description").or_else(|| query_param(&url, "error")) {
            let body = "<html><body><p>Sign-in was cancelled. You can close this tab.</p></body></html>";
            let _ = request.respond(tiny_http::Response::from_string(body).with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap(),
            ));
            return Err(error);
        }

        if let Some(code) = query_param(&url, "code") {
            let body = "<html><body><p>Instagram connected — you can close this tab and return to Reels Caption App.</p></body></html>";
            let _ = request.respond(tiny_http::Response::from_string(body).with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap(),
            ));
            return Ok(code);
        }

        // Neither -- an incidental request (e.g. favicon.ico); reply
        // plainly and keep waiting for the real one.
        let _ = request.respond(tiny_http::Response::from_string("").with_status_code(204));
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

async fn graph_get(url: &str) -> Result<serde_json::Value, String> {
    let response = reqwest::get(url).await.map_err(|e| format!("Graph API request failed: {e}"))?;
    let status = response.status();
    let body: serde_json::Value =
        response.json().await.map_err(|e| format!("Couldn't parse Graph API response: {e}"))?;
    if !status.is_success() {
        let message = body.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("Graph API error: {message}"));
    }
    Ok(body)
}

#[derive(Deserialize)]
struct PagesResponse {
    data: Vec<PageEntry>,
}

#[derive(Deserialize)]
struct PageEntry {
    id: String,
    access_token: String,
}

#[tauri::command]
pub async fn connect_instagram_account(app: AppHandle) -> Result<IgAccountSummary, String> {
    let config = load_app_config(&app)?;

    let authorize_url = format!(
        "https://www.facebook.com/dialog/oauth?client_id={}&redirect_uri={}&scope={}&response_type=code",
        urlencoding::encode(&config.app_id),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(OAUTH_SCOPES),
    );

    // Start listening before opening the browser -- a fast redirect
    // (e.g. the user already has an active Facebook session and Meta
    // skips the consent screen) must never race ahead of the listener
    // being ready.
    let listener = tokio::task::spawn_blocking(wait_for_oauth_redirect);

    tauri_plugin_opener::open_url(authorize_url, None::<&str>)
        .map_err(|e| format!("Couldn't open the system browser for sign-in: {e}"))?;

    let code = listener.await.map_err(|e| format!("Sign-in listener task panicked: {e}"))??;

    // Authorization code -> short-lived user token.
    let token_url = format!(
        "https://graph.facebook.com/oauth/access_token?client_id={}&redirect_uri={}&client_secret={}&code={}",
        urlencoding::encode(&config.app_id),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(&config.app_secret),
        urlencoding::encode(&code),
    );
    let short_lived: TokenResponse =
        serde_json::from_value(graph_get(&token_url).await?).map_err(|e| format!("Unexpected token response shape: {e}"))?;

    // Short-lived -> long-lived (~60 day) user token.
    let exchange_url = format!(
        "https://graph.facebook.com/oauth/access_token?grant_type=fb_exchange_token&client_id={}&client_secret={}&fb_exchange_token={}",
        urlencoding::encode(&config.app_id),
        urlencoding::encode(&config.app_secret),
        urlencoding::encode(&short_lived.access_token),
    );
    let long_lived: TokenResponse =
        serde_json::from_value(graph_get(&exchange_url).await?).map_err(|e| format!("Unexpected token-exchange response shape: {e}"))?;

    // Which Page(s) this user manages, each with its own Page access token.
    let pages_url = format!(
        "https://graph.facebook.com/me/accounts?access_token={}",
        urlencoding::encode(&long_lived.access_token)
    );
    let pages: PagesResponse =
        serde_json::from_value(graph_get(&pages_url).await?).map_err(|e| format!("Unexpected /me/accounts response shape: {e}"))?;

    // Find the Page (there's usually just one for a personal setup) that
    // actually has a linked Instagram Business Account.
    let mut found: Option<(String, String, String)> = None; // (page_id, page_token, ig_user_id)
    for page in &pages.data {
        let detail_url = format!(
            "https://graph.facebook.com/{}?fields=instagram_business_account&access_token={}",
            page.id,
            urlencoding::encode(&page.access_token)
        );
        let detail = graph_get(&detail_url).await?;
        if let Some(ig_id) = detail.get("instagram_business_account").and_then(|v| v.get("id")).and_then(|v| v.as_str()) {
            found = Some((page.id.clone(), page.access_token.clone(), ig_id.to_string()));
            break;
        }
    }

    let (page_id, page_access_token, ig_user_id) = found.ok_or_else(|| {
        "No Facebook Page linked to an Instagram Business/Creator account was found on this Facebook account. \
         Double-check the Page <-> Instagram link (Facebook Accounts Center) before reconnecting."
            .to_string()
    })?;

    let username_url = format!(
        "https://graph.facebook.com/{ig_user_id}?fields=username&access_token={}",
        urlencoding::encode(&page_access_token)
    );
    let username_body = graph_get(&username_url).await?;
    let username = username_body.get("username").and_then(|v| v.as_str()).unwrap_or("(unknown)").to_string();

    let account = IgAccount {
        page_access_token,
        page_id,
        ig_user_id,
        username: username.clone(),
        obtained_at: now_unix_seconds(),
        expires_in_seconds: long_lived.expires_in,
    };
    let path = account_path(&app)?;
    let contents = serde_json::to_string(&account).map_err(|e| format!("Couldn't serialize account: {e}"))?;
    std::fs::write(&path, contents).map_err(|e| format!("Couldn't save connected account: {e}"))?;

    Ok(IgAccountSummary { username })
}

/// Unused for now (kept for Phase 3's `media_host.rs`/publish flow to
/// reuse `cli_path` the same way every other subprocess/file-path call
/// site in this crate does) -- referenced here so the import above isn't
/// flagged dead code before that phase lands.
#[allow(dead_code)]
fn _touch_cli_path(p: &std::path::Path) -> String {
    cli_path(p)
}
