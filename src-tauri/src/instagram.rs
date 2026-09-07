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
// local HTTPS listener, exchange the code for tokens entirely server-side
// (well, app-side) via Graph API. No cloud intermediary, no embedded
// webview capturing credentials -- the user authenticates in their own
// real browser, where their existing Facebook session/2FA/password
// manager all just work normally.
//
// HTTPS, not HTTP -- confirmed the hard way against a real Meta app: a
// plain `http://localhost:.../callback` redirect URI is rejected outright
// (a real, longstanding Meta policy requiring OAuth redirects to be
// HTTPS, not something specific to this app or a bug to work around a
// different way). The listener below presents a self-signed certificate
// generated fresh per sign-in attempt -- nothing needs it to be *trusted*
// by anything, since the connection never leaves this machine, just
// *present*, so the browser can complete the final redirect hop. The
// browser shows a one-time "connection isn't private" interstitial for
// that hop (self-signed, unavoidable without installing a locally-trusted
// root CA on the user's machine -- a much bigger, more invasive ask this
// deliberately doesn't do); the user clicks through it once per sign-in.
//
// Publishing itself (Phase 3 in the plan) is a separate module -- this
// one only gets from "nothing connected" to "we have a Page access token
// + the linked Instagram Business Account's id, persisted locally."

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const REDIRECT_URI: &str = "https://localhost:47829/instagram/callback";
const REDIRECT_PORT: &str = "47829";
// Confirmed the hard way against a real app: `instagram_business_basic` /
// `instagram_business_content_publish` are scope names for Meta's newer,
// separate "Instagram API with Instagram Login" product (its own OAuth
// domain, not this one) -- requesting them against the classic
// `facebook.com/dialog/oauth` endpoint this app actually uses (via the
// "Facebook Login for Business" product, since our Instagram Business
// Account is discovered through a connected Facebook Page) fails with
// "Invalid Scopes". These are the correct names for *this* endpoint.
// `business_management` + `pages_read_engagement` are needed on top of the
// three above for any Page that lives inside a Business Portfolio (Meta
// Business Suite) rather than being a plain personally-owned Page --
// confirmed live: without them, /me/accounts returns zero Pages for such an
// account, and the /me/businesses -> /{id}/owned_pages fallback this app
// uses to reach them anyway 400s with "missing permission... pages_read_engagement".
const OAUTH_SCOPES: &str =
    "pages_show_list,instagram_basic,instagram_content_publish,business_management,pages_read_engagement";

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
/// `instagram::publish_reel` needs, plus the raw long-lived user token
/// (`user_access_token`) purely so `refresh_instagram_token_if_needed` can
/// extend it before it expires without asking the user to reconnect.
/// `page_access_token` (not the user token) is what Instagram's own
/// publish endpoints expect for an app using this permission model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgAccount {
    pub page_access_token: String,
    pub page_id: String,
    pub ig_user_id: String,
    pub username: String,
    /// Empty for an account connected before this field existed --
    /// `refresh_instagram_token_if_needed` treats that the same as "can't
    /// refresh, needs a manual reconnect once" rather than erroring.
    #[serde(default)]
    pub user_access_token: String,
    /// Unix seconds. `obtained_at + expires_in_seconds` is when
    /// `user_access_token` (and by extension `page_access_token`, derived
    /// from it) expires -- refreshed automatically with days to spare, see
    /// `refresh_instagram_token_if_needed`.
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

/// Generates a fresh, self-signed "localhost" certificate for the
/// listener below to present -- never cached, never installed into any
/// system trust store. It only needs to exist, not be trusted: the
/// browser's one-time "connection isn't private" interstitial is the
/// expected, documented tradeoff for that (see this module's doc
/// comment), not a bug.
fn generate_localhost_certificate() -> Result<tiny_http::SslConfig, String> {
    let rcgen::CertifiedKey { cert, key_pair } = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| format!("Couldn't generate a local TLS certificate: {e}"))?;
    Ok(tiny_http::SslConfig { certificate: cert.pem().into_bytes(), private_key: key_pair.serialize_pem().into_bytes() })
}

/// How long the local listener waits for Meta's redirect before giving up
/// and releasing the port. Confirmed the hard way this needs a bound:
/// `Server::recv()` blocks forever with no way to cancel it once
/// `spawn_blocking` has handed it to an OS thread, so a sign-in attempt
/// abandoned partway (the user closes the browser tab, or -- as actually
/// happened while building this -- Facebook's own dialog rejects the
/// request before it ever redirects anywhere) left that thread parked on
/// the socket permanently, and every subsequent "Connect Instagram" click
/// failed with "Only one usage of each socket address is normally
/// permitted" (OS error 10048) until the whole app was restarted. A
/// bounded wait via `recv_timeout` means an abandoned attempt frees the
/// port on its own within a few minutes instead of needing that.
const OAUTH_REDIRECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Waits (blocking, so run inside `spawn_blocking`) for exactly one
/// request to the redirect URI, replies with a plain "you can close this
/// tab" page, and returns the `code` query param -- or an error message
/// from the `error_description` param if the user declined consent, or a
/// timeout error if nothing arrived within `OAUTH_REDIRECT_TIMEOUT`.
fn wait_for_oauth_redirect() -> Result<String, String> {
    let ssl_config = generate_localhost_certificate()?;
    let server = tiny_http::Server::https(format!("127.0.0.1:{REDIRECT_PORT}"), ssl_config)
        .map_err(|e| format!("Couldn't start local sign-in listener on port {REDIRECT_PORT}: {e}"))?;
    let deadline = std::time::Instant::now() + OAUTH_REDIRECT_TIMEOUT;

    // Real browsers issue a handful of incidental requests (favicon.ico,
    // etc.) to a page before/after the real redirect -- keep listening
    // until we see one with an actual `code` or `error` param rather than
    // assuming the very first request is it.
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(
                "Sign-in timed out waiting for Meta's redirect (5 minutes) -- the local listener has been shut down, \
                 so it's safe to try Connect Instagram again."
                    .to_string(),
            );
        }
        let request = match server.recv_timeout(remaining) {
            Ok(Some(request)) => request,
            Ok(None) => continue, // recv_timeout's own poll interval elapsed with nothing yet -- loop re-checks the real deadline above.
            Err(e) => return Err(format!("Local sign-in listener error: {e}")),
        };
        let url = request.url().to_string();

        if let Some(error) = query_param(&url, "error_description").or_else(|| query_param(&url, "error")) {
            let body = "<html><body><p>Sign-in was cancelled. You can close this tab.</p></body></html>";
            let _ = request.respond(tiny_http::Response::from_string(body).with_header(
                tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"text/html"[..]).unwrap(),
            ));
            return Err(error);
        }

        if let Some(code) = query_param(&url, "code") {
            let body = "<html><body><p>Instagram connected — you can close this tab and return to KraftReel.App.</p></body></html>";
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

#[derive(Deserialize, Clone)]
struct PageEntry {
    id: String,
    name: String,
    access_token: String,
}

#[derive(Deserialize)]
struct BusinessesResponse {
    data: Vec<BusinessEntry>,
}

#[derive(Deserialize)]
struct BusinessEntry {
    id: String,
}

// Pages that live inside a Business Portfolio (Meta Business Suite) are
// often NOT returned by /me/accounts for a plain personal user token, even
// with pages_show_list granted and full Page access -- confirmed live
// against a real account where /me/accounts came back empty despite the
// account owning the Page outright. /{business_id}/owned_pages is the
// documented way to reach those same Pages directly.
async fn discover_pages_via_business_portfolios(user_token: &str) -> Vec<PageEntry> {
    let businesses_url = format!(
        "https://graph.facebook.com/me/businesses?access_token={}",
        urlencoding::encode(user_token)
    );
    let Ok(businesses_body) = graph_get(&businesses_url).await else {
        return Vec::new();
    };
    let Ok(businesses) = serde_json::from_value::<BusinessesResponse>(businesses_body) else {
        return Vec::new();
    };

    let mut pages = Vec::new();
    for business in businesses.data {
        let owned_pages_url = format!(
            "https://graph.facebook.com/{}/owned_pages?fields=id,name,access_token&access_token={}",
            business.id,
            urlencoding::encode(user_token)
        );
        if let Ok(body) = graph_get(&owned_pages_url).await {
            if let Ok(response) = serde_json::from_value::<PagesResponse>(body) {
                pages.extend(response.data);
            }
        }
    }
    pages
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

    let discovered = discover_ig_account_from_user_token(&long_lived.access_token).await?;

    let account = IgAccount {
        page_access_token: discovered.page_access_token,
        page_id: discovered.page_id,
        ig_user_id: discovered.ig_user_id,
        username: discovered.username.clone(),
        user_access_token: long_lived.access_token,
        obtained_at: now_unix_seconds(),
        expires_in_seconds: long_lived.expires_in,
    };
    save_account(&app, &account)?;

    Ok(IgAccountSummary { username: discovered.username })
}

struct DiscoveredIgAccount {
    page_id: String,
    page_access_token: String,
    ig_user_id: String,
    username: String,
}

/// Everything between "have a long-lived user token" and "know which Page
/// + linked Instagram Business Account it grants access to" -- shared by
/// the initial connect flow and `refresh_instagram_token_if_needed`, which
/// needs to re-derive a fresh Page access token from a freshly-refreshed
/// user token the same way.
async fn discover_ig_account_from_user_token(user_token: &str) -> Result<DiscoveredIgAccount, String> {
    // Which Page(s) this user manages, each with its own Page access token.
    let pages_url =
        format!("https://graph.facebook.com/me/accounts?access_token={}", urlencoding::encode(user_token));
    let mut pages: PagesResponse =
        serde_json::from_value(graph_get(&pages_url).await?).map_err(|e| format!("Unexpected /me/accounts response shape: {e}"))?;

    if pages.data.is_empty() {
        pages.data = discover_pages_via_business_portfolios(user_token).await;
    }

    if pages.data.is_empty() {
        // Meta can silently drop a requested scope from the actual grant
        // (no error at consent time) rather than reject it outright --
        // /me/permissions shows what the token *actually* ended up with,
        // which is the concrete way to tell "pages_show_list wasn't
        // granted" apart from "granted, but this account genuinely has no
        // Pages" instead of guessing between the two again.
        let permissions_url =
            format!("https://graph.facebook.com/me/permissions?access_token={}", urlencoding::encode(user_token));
        let granted = match graph_get(&permissions_url).await {
            Ok(body) => body
                .get("data")
                .and_then(|d| d.as_array())
                .map(|entries| {
                    entries
                        .iter()
                        .filter(|e| e.get("status").and_then(|s| s.as_str()) == Some("granted"))
                        .filter_map(|e| e.get("permission").and_then(|p| p.as_str()))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_else(|| "(couldn't parse /me/permissions response)".to_string()),
            Err(e) => format!("(couldn't check: {e})"),
        };
        return Err(format!(
            "No Facebook Pages were returned for this account, either directly or through any Business Portfolio \
             it belongs to. Permissions actually granted on this token: [{granted}]. \
             If `pages_show_list` isn't in that list, sign-in dropped it silently -- try disconnecting this app's \
             access under Facebook -> Settings -> Business Integrations (or Apps and Websites) and reconnecting from \
             scratch, so Facebook re-prompts for every requested permission instead of reusing an old grant."
        ));
    }

    // Find the Page (there's usually just one for a personal setup) that
    // actually has a linked Instagram Business Account. Diagnostic names
    // collected along the way so a "none found" error can say exactly
    // which Page(s) were checked, rather than a generic dead end.
    let mut found: Option<(String, String, String)> = None; // (page_id, page_token, ig_user_id)
    let mut checked_names: Vec<String> = Vec::new();
    for page in &pages.data {
        checked_names.push(page.name.clone());
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
        format!(
            "Checked {} Facebook Page(s) ({}) but none has a linked Instagram Business/Creator account, according \
             to the Graph API. If you linked it via Accounts Center, also check the Page's own \
             Settings -> Linked accounts directly -- the two haven't always been fully in sync for every account.",
            checked_names.len(),
            checked_names.join(", ")
        )
    })?;

    let username_url = format!(
        "https://graph.facebook.com/{ig_user_id}?fields=username&access_token={}",
        urlencoding::encode(&page_access_token)
    );
    let username_body = graph_get(&username_url).await?;
    let username = username_body.get("username").and_then(|v| v.as_str()).unwrap_or("(unknown)").to_string();

    Ok(DiscoveredIgAccount { page_id, page_access_token, ig_user_id, username })
}

fn save_account(app: &AppHandle, account: &IgAccount) -> Result<(), String> {
    let path = account_path(app)?;
    let contents = serde_json::to_string(account).map_err(|e| format!("Couldn't serialize account: {e}"))?;
    std::fs::write(&path, contents).map_err(|e| format!("Couldn't save connected account: {e}"))
}

/// How long before `user_access_token` actually expires to proactively
/// refresh it -- Meta's `fb_exchange_token` grant can extend a long-lived
/// token that still has time left on it (effectively resetting its ~60-day
/// clock), but can't revive one that's already expired, so this needs
/// enough margin to comfortably survive the app being closed/asleep for a
/// while around the deadline. Checked every scheduler tick (`scheduler.rs`,
/// every 60s) -- cheap once refreshed (a plain timestamp comparison, no
/// network call) until actually within this window again in ~55 days.
const TOKEN_REFRESH_THRESHOLD_SECONDS: u64 = 5 * 24 * 60 * 60;

/// Best-effort: refreshes the connected account's token if it's within
/// `TOKEN_REFRESH_THRESHOLD_SECONDS` of expiring, so a long-running
/// (tray-resident) install never needs a manual reconnect as long as it's
/// launched at least once every ~55 days. A no-op (`Ok(())`) whenever
/// there's nothing to do: no account connected, or an account connected
/// before `user_access_token` existed (needs one manual reconnect to start
/// benefiting from this), or simply not near expiry yet. Only returns an
/// error for an actual refresh attempt that failed -- callers treat that
/// as non-fatal background maintenance, not something to surface loudly;
/// if the token does fully expire, `publish_reel`'s own check gives a
/// clear, actionable error at the point that actually matters (a real
/// publish attempt).
pub async fn refresh_instagram_token_if_needed(app: &AppHandle) -> Result<(), String> {
    let Ok(account) = load_account(app) else {
        return Ok(());
    };
    if account.user_access_token.is_empty() {
        return Ok(());
    }
    let expires_at = account.obtained_at + account.expires_in_seconds;
    let now = now_unix_seconds();
    if expires_at.saturating_sub(now) > TOKEN_REFRESH_THRESHOLD_SECONDS {
        return Ok(());
    }

    let config = load_app_config(app)?;
    let exchange_url = format!(
        "https://graph.facebook.com/oauth/access_token?grant_type=fb_exchange_token&client_id={}&client_secret={}&fb_exchange_token={}",
        urlencoding::encode(&config.app_id),
        urlencoding::encode(&config.app_secret),
        urlencoding::encode(&account.user_access_token),
    );
    let refreshed: TokenResponse = serde_json::from_value(graph_get(&exchange_url).await?)
        .map_err(|e| format!("Unexpected token-refresh response shape: {e}"))?;

    let discovered = discover_ig_account_from_user_token(&refreshed.access_token).await?;

    let updated = IgAccount {
        page_access_token: discovered.page_access_token,
        page_id: discovered.page_id,
        ig_user_id: discovered.ig_user_id,
        username: discovered.username,
        user_access_token: refreshed.access_token,
        obtained_at: now_unix_seconds(),
        expires_in_seconds: refreshed.expires_in,
    };
    save_account(app, &updated)
}

fn load_account(app: &AppHandle) -> Result<IgAccount, String> {
    let path = account_path(app)?;
    let contents = std::fs::read_to_string(&path)
        .map_err(|_| "No Instagram account connected -- connect one in Settings first.".to_string())?;
    serde_json::from_str(&contents).map_err(|e| format!("Couldn't read saved Instagram account: {e}"))
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishResult {
    pub media_id: String,
}

#[derive(Deserialize)]
struct ContainerCreateResponse {
    id: String,
}

#[derive(Deserialize)]
struct ContainerStatusResponse {
    status_code: String,
    #[serde(default)]
    status: Option<String>,
}

#[derive(Deserialize)]
struct MediaPublishResponse {
    id: String,
}

/// Same error-shape handling as `graph_get`, but for a `reqwest::Response`
/// already in hand -- the POST calls below build their own request (for
/// form params) rather than going through the plain-GET helper.
async fn graph_response_json(response: reqwest::Response) -> Result<serde_json::Value, String> {
    let status = response.status();
    let body: serde_json::Value = response.json().await.map_err(|e| format!("Couldn't parse Graph API response: {e}"))?;
    if !status.is_success() {
        let message = body.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("Graph API error: {message}"));
    }
    Ok(body)
}

/// How long to keep polling a media container for `FINISHED` before giving
/// up -- Instagram's own video processing for a Reel-length clip is
/// normally done well within this, but a slow tunnel upload or a busy
/// processing queue on Meta's side can take longer.
const CONTAINER_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const CONTAINER_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

async fn publish_via_graph(account: &IgAccount, video_url: &str, caption: &str) -> Result<PublishResult, String> {
    let client = reqwest::Client::new();

    let create_url = format!("https://graph.facebook.com/{}/media", account.ig_user_id);
    let create_resp = client
        .post(&create_url)
        .form(&[
            ("media_type", "REELS"),
            ("video_url", video_url),
            ("caption", caption),
            ("access_token", account.page_access_token.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("Couldn't create Instagram media container: {e}"))?;
    let container: ContainerCreateResponse = serde_json::from_value(graph_response_json(create_resp).await?)
        .map_err(|e| format!("Unexpected container-create response shape: {e}"))?;

    let deadline = std::time::Instant::now() + CONTAINER_POLL_TIMEOUT;
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(
                "Timed out waiting for Instagram to finish processing the video (10 minutes) -- it may still \
                 complete on Meta's side; check the account's Instagram app."
                    .to_string(),
            );
        }
        let status_url = format!(
            "https://graph.facebook.com/{}?fields=status_code&access_token={}",
            container.id,
            urlencoding::encode(&account.page_access_token)
        );
        let status: ContainerStatusResponse = serde_json::from_value(graph_get(&status_url).await?)
            .map_err(|e| format!("Unexpected container-status response shape: {e}"))?;
        match status.status_code.as_str() {
            "FINISHED" => break,
            "ERROR" | "EXPIRED" => {
                return Err(format!(
                    "Instagram failed to process the video (status: {}{}).",
                    status.status_code,
                    status.status.map(|s| format!(" -- {s}")).unwrap_or_default()
                ))
            }
            _ => tokio::time::sleep(CONTAINER_POLL_INTERVAL).await,
        }
    }

    let publish_url = format!("https://graph.facebook.com/{}/media_publish", account.ig_user_id);
    let publish_resp = client
        .post(&publish_url)
        .form(&[("creation_id", container.id.as_str()), ("access_token", account.page_access_token.as_str())])
        .send()
        .await
        .map_err(|e| format!("Couldn't publish the processed media: {e}"))?;
    let published: MediaPublishResponse = serde_json::from_value(graph_response_json(publish_resp).await?)
        .map_err(|e| format!("Unexpected publish response shape: {e}"))?;

    Ok(PublishResult { media_id: published.id })
}

/// Hosts `video_path` temporarily (see `media_host.rs`), runs it through
/// Instagram's container -> poll -> publish flow, and always tears the
/// temporary hosting down afterward regardless of the publish outcome.
/// Used both by the immediate "Post now" command below and by
/// `scheduler.rs`'s once-a-minute tick for due scheduled posts.
pub async fn publish_reel(app: &AppHandle, video_path: &str, caption: &str) -> Result<PublishResult, String> {
    let account = load_account(app)?;

    let expires_at = account.obtained_at + account.expires_in_seconds;
    if now_unix_seconds() >= expires_at {
        return Err("Your Instagram connection has expired (tokens last ~60 days) -- reconnect it in Settings.".to_string());
    }

    let hosted = crate::media_host::host_video_temporarily(app, std::path::Path::new(video_path)).await?;
    let result = publish_via_graph(&account, &hosted.public_url, caption).await;
    hosted.stop().await;
    result
}

#[tauri::command]
pub async fn post_to_instagram_now(app: AppHandle, video_path: String, caption: String) -> Result<PublishResult, String> {
    publish_reel(&app, &video_path, &caption).await
}
