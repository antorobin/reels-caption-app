// Local LLM service: llama.cpp's own `llama-server.exe`, running
// Qwen2.5-0.5B-Instruct (GGUF, Q4_K_M quant) -- the only piece of this
// app's stack that talks to a model over HTTP rather than a one-shot
// subprocess call, because reloading a model on every request would make
// even a single suggestion take as long as the model load itself.
// `llama-server` is started once, lazily, on first use, and reused for the
// life of the app.
//
// Chosen over the previously-used Sarvam-1 (2B, base/completion model):
// Sarvam-1 wasn't instruction-tuned, which meant every feature built on it
// needed a hand-rolled few-shot prompt and fragile line-based text
// parsing -- workable for a single hook line, but a real liability for
// content_ideas.rs's structured multi-field output (title/description/
// hashtags/emoji). Qwen2.5-0.5B-Instruct is genuinely instruction-tuned,
// small enough to be meaningfully faster and lighter on this project's
// consistently RAM-constrained dev machine, and Apache-2.0 licensed.
//
// Every call formats Qwen's own ChatML template
// (`<|im_start|>role\n...<|im_end|>`) by hand and hits the plain
// `/completion` endpoint with a `json_schema` constraint, rather than
// `/v1/chat/completions` -- this keeps the exact prompt shape, stop
// token, and schema fully under this crate's control instead of depending
// on an OpenAI-compatible endpoint's request shape matching what a
// particular llama.cpp build expects.
//
// Bundled binary (resources/llama/, see README) -- llama.cpp's official
// CPU-only Windows x64 release build, no CUDA/GPU dependency, matching
// this project's "optimized for a laptop's CPU" goal for the LLM piece
// too.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const LLAMA_SERVER_PORT: u16 = 8734;
const LLAMA_SERVER_HOST: &str = "127.0.0.1";

static SERVER_HANDLE: Mutex<Option<Child>> = Mutex::const_new(None);

fn resource_path(app: &AppHandle, rel: &str) -> Option<PathBuf> {
    // A downloaded runtime component (`runtime_fetch.rs`) -- the slim
    // installer's path -- takes precedence over a bundled copy so a
    // fetched update wins over a stale bundled one.
    let downloaded = crate::runtime_fetch::runtime_dir().join(rel);
    if downloaded.exists() {
        return Some(downloaded);
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join(rel);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    let dev_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join(rel);
    dev_candidate.exists().then_some(dev_candidate)
}

fn llama_server_exe(app: &AppHandle) -> Result<PathBuf, String> {
    resource_path(app, "llama/llama-server.exe").ok_or_else(|| {
        "llama-server.exe not found in resources/llama/ — see README's LLM service setup section".to_string()
    })
}

fn llm_model_path(app: &AppHandle) -> Result<PathBuf, String> {
    resource_path(app, "llama/qwen2.5-0.5b-instruct-q4_k_m.gguf").ok_or_else(|| {
        "qwen2.5-0.5b-instruct-q4_k_m.gguf not found in resources/llama/ — see README's LLM service setup section"
            .to_string()
    })
}

fn base_url() -> String {
    format!("http://{LLAMA_SERVER_HOST}:{LLAMA_SERVER_PORT}")
}

async fn is_server_healthy() -> bool {
    reqwest::get(format!("{}/health", base_url())).await.is_ok_and(|r| r.status().is_success())
}

/// Starts `llama-server` if it isn't already running, and waits for it to
/// report healthy. Idempotent and safe to call before every request — the
/// common case (already running) is just one fast HTTP health check.
async fn ensure_server_running(app: &AppHandle) -> Result<(), String> {
    if is_server_healthy().await {
        return Ok(());
    }

    let mut guard = SERVER_HANDLE.lock().await;
    if is_server_healthy().await {
        return Ok(());
    }

    let exe = llama_server_exe(app)?;
    let model = llm_model_path(app)?;

    let child = Command::new(&exe)
        .args([
            "--model",
            &model.to_string_lossy(),
            "--host",
            LLAMA_SERVER_HOST,
            "--port",
            &LLAMA_SERVER_PORT.to_string(),
            "--ctx-size",
            "4096",
            "--no-webui",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start llama-server: {e}"))?;
    *guard = Some(child);
    drop(guard);

    // Model load (well under a second for a ~400MB Q4_K_M quant on CPU)
    // happens before llama-server starts listening at all, so poll rather
    // than assume any fixed startup time.
    for _ in 0..60 {
        if is_server_healthy().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err("llama-server didn't become healthy within 30s".to_string())
}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    prompt: String,
    n_predict: i32,
    temperature: f32,
    stop: &'a [&'a str],
    #[serde(skip_serializing_if = "Option::is_none")]
    json_schema: Option<Value>,
}

#[derive(Deserialize)]
struct CompletionResponse {
    content: String,
}

/// Runs `user_prompt` (with `system_prompt` as the system turn) through
/// Qwen2.5-0.5B-Instruct, formatted in its own ChatML template. `<|im_end|>`
/// is Qwen's turn-end token — the natural stop sequence for an
/// instruction-tuned model, unlike the artificial stop markers the old
/// base-model prompt needed. `json_schema`, when set, constrains generation
/// to valid JSON matching that schema (see content_ideas.rs) so response
/// parsing never has to guess at the model's formatting. `temperature`
/// matters more at this model size than it would for a larger one --
/// confirmed directly on tts.rs's emotion classifier: a low temperature
/// alone didn't fix a real misclassification (an obviously excited script
/// still came back "calm"), but it's still the right default for a
/// single-best-label task like that versus the higher temperature a
/// creative-generation call like content_ideas.rs wants.
pub async fn complete(
    app: &AppHandle,
    system_prompt: &str,
    user_prompt: &str,
    max_tokens: i32,
    temperature: f32,
    json_schema: Option<Value>,
) -> Result<String, String> {
    ensure_server_running(app).await?;

    let prompt = format!(
        "<|im_start|>system\n{system_prompt}<|im_end|>\n<|im_start|>user\n{user_prompt}<|im_end|>\n<|im_start|>assistant\n"
    );

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/completion", base_url()))
        .json(&CompletionRequest { prompt, n_predict: max_tokens, temperature, stop: &["<|im_end|>"], json_schema })
        .send()
        .await
        .map_err(|e| format!("Failed to reach llama-server: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("llama-server returned {status}: {body}"));
    }

    let parsed: CompletionResponse =
        response.json().await.map_err(|e| format!("Couldn't parse llama-server response: {e}"))?;
    Ok(parsed.content)
}

/// Not currently called anywhere — `proc_cleanup`'s Job Object already
/// guarantees `llama-server` dies with the app on exit, the same way every
/// other subprocess in this crate does, so there's no separate app-exit
/// hook needed. Kept as a callable for a future "stop the LLM service"
/// UI action if one turns out to be worth adding.
#[allow(dead_code)]
pub async fn shutdown() {
    if let Some(mut child) = SERVER_HANDLE.lock().await.take() {
        let _ = child.kill().await;
    }
}
