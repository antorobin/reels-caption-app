// Local text-embedding service for the project library's semantic search
// (library.rs's `search_projects`): a small Python HTTP server
// (tts/embed_server.py) running `all-MiniLM-L6-v2` (via `sentence-
// transformers`) in the existing "tts" conda env -- reused rather than a
// new env, same reasoning as music_gen.rs reusing it for MusicGen (it
// already has `torch`+`transformers`, and `sentence-transformers` is a thin
// wrapper over both).
//
// Started once, lazily, on first use, and kept running for the app's whole
// lifetime -- the exact same shape as llm.rs's llama-server, and for the
// same reason: loading the model takes ~19s (confirmed directly), which is
// fine to pay once but not on every search keystroke. This is deliberately
// NOT the one-shot-subprocess-per-call pattern most of this app's other
// Python integrations use (tts.rs, music_gen.rs) -- those are all
// multi-second-to-minutes operations already, where a fresh process's
// overhead is noise; embedding a query while the user is actively typing
// a search box is not.
//
// A different port (8735) from llm.rs's llama-server (8734) -- two
// independent local model servers, no shared state.

use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tauri::AppHandle;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::tts::{python_exe, resolve_tts_prefix, tts_script_path};
use crate::util::cli_path;

const EMBED_SERVER_PORT: u16 = 8735;
const EMBED_SERVER_HOST: &str = "127.0.0.1";

static SERVER_HANDLE: Mutex<Option<Child>> = Mutex::const_new(None);

fn base_url() -> String {
    format!("http://{EMBED_SERVER_HOST}:{EMBED_SERVER_PORT}")
}

async fn is_server_healthy() -> bool {
    reqwest::get(format!("{}/health", base_url())).await.is_ok_and(|r| r.status().is_success())
}

/// Starts the embedding server if it isn't already running, and waits for
/// it to report healthy. Idempotent and safe to call before every request --
/// the common case (already running) is just one fast HTTP health check,
/// same as llm.rs's `ensure_server_running`.
// `_app` is unused today (the script path and conda env are both resolved
// without one) but kept in the signature to mirror llm.rs's exact shape --
// a future resource-bundled fallback for the embedding model would need it.
async fn ensure_server_running(_app: &AppHandle) -> Result<(), String> {
    if is_server_healthy().await {
        return Ok(());
    }

    let mut guard = SERVER_HANDLE.lock().await;
    if is_server_healthy().await {
        return Ok(());
    }

    let prefix = resolve_tts_prefix().await?;
    let python = python_exe(&prefix);
    let script = tts_script_path("embed_server.py");

    let child = Command::new(&python)
        .arg(cli_path(&script))
        .arg(EMBED_SERVER_PORT.to_string())
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            format!(
                "Embedding search engine not found ({e}). Set up the 'tts' conda environment \
                 (see README) with `pip install sentence-transformers` -- or if it's already \
                 installed somewhere this couldn't find, set REELS_CAPTION_APP_TTS_CONDA_PATH."
            )
        })?;
    *guard = Some(child);
    drop(guard);

    // The model load itself is the ~19s the module doc comment mentions --
    // poll rather than assume any fixed startup time.
    for _ in 0..60 {
        if is_server_healthy().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("The embedding search engine didn't become ready within 30s".to_string())
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    text: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct EmbedError {
    error: String,
}

/// Embeds `text` into a 384-dim vector (all-MiniLM-L6-v2's native output
/// size) for cosine-similarity search. Starts the embedding server on
/// first call; subsequent calls just hit the already-running one.
pub async fn embed(app: &AppHandle, text: &str) -> Result<Vec<f32>, String> {
    ensure_server_running(app).await?;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/embed", base_url()))
        .json(&EmbedRequest { text })
        .send()
        .await
        .map_err(|e| format!("Failed to reach the embedding server: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.json::<EmbedError>().await.map(|e| e.error).unwrap_or_default();
        return Err(format!("Embedding server returned {status}: {body}"));
    }

    let parsed: EmbedResponse =
        response.json().await.map_err(|e| format!("Couldn't parse the embedding server's response: {e}"))?;
    Ok(parsed.embedding)
}
