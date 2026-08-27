// Temporary public hosting for one video file so Instagram's Graph API can
// fetch it while creating a media container -- Instagram's publish flow
// requires a public https:// URL, not a direct upload, and this app has no
// permanent cloud storage. A tiny local static-file server (`tiny_http` --
// already a dependency for the OAuth listener in instagram.rs, sync and
// minimal, no standing async-runtime overhead of its own) is fronted by a
// temporary `cloudflared` quick tunnel (no Cloudflare account needed) for
// just the seconds/minutes one publish actually takes; both are torn down
// via `HostedMedia::stop` as soon as the caller is done with the URL, so
// there is zero standing cost between posts.
//
// Known limitation: the server always returns the whole file with a plain
// 200 OK, ignoring `Range` request headers. Meta's video fetcher has been
// observed working fine against a plain full-file GET for Reel-length
// clips; revisit if a much larger file ever needs partial/resumable
// fetches.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::util::cli_path;

const LOCAL_MEDIA_PORT: &str = "47831";

/// Handle to a running local-file-server + tunnel pair. `Drop` also tears
/// both down (best-effort, non-blocking) in case a caller forgets to await
/// `stop`, but `stop` is preferred since it can actually be awaited.
pub struct HostedMedia {
    pub public_url: String,
    shutdown: Arc<AtomicBool>,
    server_thread: Option<std::thread::JoinHandle<()>>,
    tunnel_child: Option<Child>,
}

impl HostedMedia {
    pub async fn stop(mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(mut child) = self.tunnel_child.take() {
            let _ = child.kill().await;
        }
        if let Some(handle) = self.server_thread.take() {
            let _ = tokio::task::spawn_blocking(move || handle.join()).await;
        }
    }
}

impl Drop for HostedMedia {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(child) = &mut self.tunnel_child {
            let _ = child.start_kill();
        }
    }
}

/// How long the local file server keeps itself alive waiting for
/// Instagram's fetch, even if `stop()` is never called for some reason
/// (e.g. the calling task panics) -- mirrors the same bounded-wait pattern
/// `instagram.rs`'s OAuth listener uses, so an abandoned publish attempt
/// can't leave a server thread (or its port) parked forever.
const SERVER_MAX_LIFETIME: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn serve_file_until_shutdown(server: tiny_http::Server, file_bytes: Arc<Vec<u8>>, shutdown: Arc<AtomicBool>) {
    let deadline = std::time::Instant::now() + SERVER_MAX_LIFETIME;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return;
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return;
        }
        let poll_wait = remaining.min(std::time::Duration::from_secs(1));
        match server.recv_timeout(poll_wait) {
            Ok(Some(request)) => {
                let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"video/mp4"[..]).unwrap();
                let response = tiny_http::Response::from_data(file_bytes.as_slice().to_vec()).with_header(header);
                let _ = request.respond(response);
            }
            Ok(None) => continue,
            Err(_) => return,
        }
    }
}

fn cloudflared_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "cloudflared.exe"
    } else {
        "cloudflared"
    }
}

fn cloudflared_cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("Couldn't resolve app data directory: {e}"))?;
    let bin_dir = dir.join("bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| format!("Couldn't create {}: {e}", bin_dir.display()))?;
    Ok(bin_dir.join(cloudflared_filename()))
}

/// Downloads cloudflared straight from its own GitHub Releases -- same
/// on-demand-fetch idea as `model_fetch.rs`, but for a small (~30-60MB)
/// single-purpose binary most users won't otherwise have installed, so
/// bundling it into every build's installer isn't worth the size for a
/// feature (scheduled posting) most installs may never use.
async fn download_cloudflared(dest: &Path) -> Result<(), String> {
    let (asset, is_archive) = if cfg!(target_os = "windows") {
        ("cloudflared-windows-amd64.exe", false)
    } else if cfg!(target_os = "macos") {
        ("cloudflared-darwin-amd64.tgz", true)
    } else {
        ("cloudflared-linux-amd64", false)
    };
    let url = format!("https://github.com/cloudflare/cloudflared/releases/latest/download/{asset}");
    let response = reqwest::get(&url).await.map_err(|e| format!("Couldn't download cloudflared: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Couldn't download cloudflared: HTTP {}", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| format!("Couldn't read cloudflared download: {e}"))?;

    if is_archive {
        let archive_path = dest.with_extension("tgz");
        std::fs::write(&archive_path, &bytes).map_err(|e| format!("Couldn't write cloudflared archive: {e}"))?;
        let dest_dir = dest.parent().ok_or("Invalid cloudflared destination path")?;
        let output = std::process::Command::new("tar")
            .args(["-xzf", &cli_path(&archive_path), "-C", &cli_path(dest_dir)])
            .output()
            .map_err(|e| format!("Failed to run tar to extract cloudflared: {e}"))?;
        let _ = std::fs::remove_file(&archive_path);
        if !output.status.success() {
            return Err(format!("Failed to extract cloudflared: {}", String::from_utf8_lossy(&output.stderr)));
        }
        // The macOS release archive contains a plain `cloudflared` binary at its root.
        let extracted = dest_dir.join("cloudflared");
        if extracted != dest {
            std::fs::rename(&extracted, dest).map_err(|e| format!("Couldn't finalize cloudflared binary: {e}"))?;
        }
    } else {
        std::fs::write(dest, &bytes).map_err(|e| format!("Couldn't write cloudflared binary: {e}"))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms =
            std::fs::metadata(dest).map_err(|e| format!("Couldn't read cloudflared permissions: {e}"))?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(dest, perms).map_err(|e| format!("Couldn't make cloudflared executable: {e}"))?;
    }

    Ok(())
}

async fn resolve_cloudflared_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("REELS_CAPTION_APP_CLOUDFLARED") {
        return Ok(PathBuf::from(path));
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("bin").join(cloudflared_filename());
        if bundled.exists() {
            return Ok(bundled);
        }
    }
    let cached = cloudflared_cache_path(app)?;
    if cached.exists() {
        return Ok(cached);
    }
    // A system-wide install (e.g. `brew install cloudflared`) resolves via
    // PATH without needing a fetch at all.
    let on_path = Command::new("cloudflared")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false);
    if on_path {
        return Ok(PathBuf::from("cloudflared"));
    }
    download_cloudflared(&cached).await?;
    Ok(cached)
}

/// cloudflared logs its quick-tunnel URL (`https://<random-words>.trycloudflare.com`)
/// as part of a decorative bordered box on stderr, not stdout -- but which
/// stream carries it isn't documented as stable across versions, so both
/// are scanned concurrently via two background tasks racing into one
/// channel; whichever finds it first wins.
async fn find_tunnel_url(stdout: tokio::process::ChildStdout, stderr: tokio::process::ChildStderr) -> Option<String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(1);

    let tx_stdout = tx.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = extract_trycloudflare_url(&line) {
                let _ = tx_stdout.send(url).await;
                return;
            }
        }
    });

    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(url) = extract_trycloudflare_url(&line) {
                let _ = tx.send(url).await;
                return;
            }
        }
    });

    rx.recv().await
}

fn extract_trycloudflare_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let candidate = &line[start..];
    let end = candidate.find(char::is_whitespace).unwrap_or(candidate.len());
    let url = candidate[..end].trim_end_matches(|c: char| !c.is_alphanumeric() && c != '/');
    if url.contains(".trycloudflare.com") {
        Some(url.to_string())
    } else {
        None
    }
}

/// Serves `video_path` locally and fronts it with a temporary `cloudflared`
/// quick tunnel so Instagram's Graph API (which fetches video from a
/// public URL rather than accepting a direct upload) can reach it. Both
/// the local server and the tunnel are torn down by `HostedMedia::stop`
/// (or its `Drop` impl) as soon as the caller is done with the URL.
pub async fn host_video_temporarily(app: &AppHandle, video_path: &Path) -> Result<HostedMedia, String> {
    let file_bytes =
        Arc::new(std::fs::read(video_path).map_err(|e| format!("Couldn't read video file to host it: {e}"))?);

    let server = tiny_http::Server::http(format!("127.0.0.1:{LOCAL_MEDIA_PORT}"))
        .map_err(|e| format!("Couldn't start local media server on port {LOCAL_MEDIA_PORT}: {e}"))?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let server_thread = {
        let shutdown = shutdown.clone();
        std::thread::spawn(move || serve_file_until_shutdown(server, file_bytes, shutdown))
    };

    let cloudflared_path = resolve_cloudflared_path(app).await?;
    let mut child = Command::new(&cloudflared_path)
        .args(["tunnel", "--url", &format!("http://127.0.0.1:{LOCAL_MEDIA_PORT}"), "--no-autoupdate"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Couldn't start cloudflared ({}): {e}", cloudflared_path.display()))?;

    let stdout = child.stdout.take().ok_or("cloudflared has no stdout")?;
    let stderr = child.stderr.take().ok_or("cloudflared has no stderr")?;

    let public_url = match tokio::time::timeout(std::time::Duration::from_secs(30), find_tunnel_url(stdout, stderr)).await
    {
        Ok(Some(url)) => url,
        Ok(None) => {
            shutdown.store(true, Ordering::SeqCst);
            let _ = child.start_kill();
            return Err("cloudflared exited without reporting a tunnel URL.".to_string());
        }
        Err(_) => {
            shutdown.store(true, Ordering::SeqCst);
            let _ = child.start_kill();
            return Err("Timed out waiting for cloudflared to open a tunnel (30 seconds).".to_string());
        }
    };

    Ok(HostedMedia { public_url, shutdown, server_thread: Some(server_thread), tunnel_child: Some(child) })
}
