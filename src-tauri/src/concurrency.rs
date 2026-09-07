// Two process-wide semaphores gating the operations that genuinely compete
// for a scarce resource once more than one project can be processed at
// once -- introduced alongside the per-project state store (see
// src/state/projectStore.js) that made concurrent processing possible in
// the first place. Before this, only one project was ever "live," so
// nothing needed gating; every `#[tauri::command] async fn` in this crate
// runs as an independent, uncapped tokio task otherwise (confirmed via
// grep before writing this -- no Semaphore, no custom runtime, no
// concurrency limit existed anywhere in this codebase).
//
// Two separate gates, not one, because they protect genuinely different
// resources with different failure modes:
//
// - `heavy_ml_semaphore()`: STT transcription (pipeline.rs/
//   mixed_language.rs), TTS voiceover generation (tts.rs), and MusicGen
//   (music_gen.rs) each spawn a *fresh subprocess per call* that loads its
//   own model into memory independently -- nothing is shared across calls
//   the way llm.rs's own singleton server already shares one loaded
//   model. Left unbounded, N concurrent calls means N full model
//   loads stacked in RAM at once, on this app's own stated "RAM-
//   constrained laptop" target hardware. Failure mode: OOM/swapping, a
//   slow degradation, not a hard error.
// - `encode_semaphore()`: ffmpeg-encode-ending commands (burn_captions,
//   duck_music, remove_silence_and_fillers, normalize_audio,
//   sync_voice_over) all end by encoding through whichever hardware
//   encoder `ffmpeg::best_encoder()` picked for the whole app. Consumer
//   GPUs enforce a driver-level cap on concurrent encode *sessions*
//   (commonly 2-3 on consumer NVENC) -- failure mode here is a hard
//   encode error past that cap, not just slowness, and it isn't CPU/RAM
//   bound the way the ML semaphore's operations are, so scaling it with
//   core count would be meaningless -- more cores don't buy more
//   concurrent hardware encoder sessions.
//
// llm.rs (llama-server) deliberately gets NO semaphore here -- it's
// already a singleton background HTTP server that naturally serializes
// concurrent requests one-at-a-time at the HTTP layer. Adding a semaphore
// on top would just be a second, subtly different queuing mechanism
// stacked on an already-correct one. (An embedding server used to live
// here too, for the project library's semantic search -- removed once
// that search was replaced with SQLite's own FTS5, which runs
// synchronously in-process and needs no server, semaphore, or queuing of
// any kind.)

use std::sync::OnceLock;
use tokio::sync::{Semaphore, SemaphorePermit};

static HEAVY_ML_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
static ENCODE_SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();

/// `(cores / 2).clamp(1, 3)` -- auto-detected from the machine rather than
/// a fixed constant or a Settings control, per explicit request. Capped at
/// 3 regardless of core count: this bounds *simultaneous model loads*, and
/// even a high-core-count machine is unlikely to have proportionally more
/// spare RAM for stacking several independent model loads at once.
fn heavy_ml_semaphore() -> &'static Semaphore {
    HEAVY_ML_SEMAPHORE.get_or_init(|| {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let permits = (cores / 2).clamp(1, 3);
        Semaphore::new(permits)
    })
}

/// Fixed at 2, deliberately not scaled with core count -- see module doc
/// comment above for why this gates a different resource than the ML
/// semaphore.
fn encode_semaphore() -> &'static Semaphore {
    ENCODE_SEMAPHORE.get_or_init(|| Semaphore::new(2))
}

/// Acquires a heavy-ML-op permit, held for as long as the returned guard
/// stays in scope (normal Rust drop -- no manual release needed). Callers
/// just do `let _permit = concurrency::acquire_heavy_ml().await;` at the
/// top of the section that actually spawns the subprocess.
pub async fn acquire_heavy_ml() -> SemaphorePermit<'static> {
    // `.unwrap()` is safe here: this semaphore is never `close()`d, so
    // `acquire()` can't return the `Closed` error its signature allows for.
    heavy_ml_semaphore().acquire().await.unwrap()
}

/// Same shape as `acquire_heavy_ml`, for the encode gate.
pub async fn acquire_encode() -> SemaphorePermit<'static> {
    encode_semaphore().acquire().await.unwrap()
}
