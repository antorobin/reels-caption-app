// Saves a microphone clip recorded in the frontend via the standard Web
// `MediaRecorder` API (see VoiceoverSection.jsx's "Record from mic" mode),
// converts it to the same 16kHz mono WAV format the rest of this pipeline
// already expects, and cleans it up: trims dead air off the front (the gap
// between clicking "Start recording" and actually starting to speak is real
// silence, not part of anything anyone wants transcribed or used as a
// voice-over) and runs a light denoise/rumble-cut/loudness pass so a mic
// recording holds up next to the app's other, studio-cleaner audio paths
// (TTS output, an uploaded voice-over).
//
// Capturing the microphone itself deliberately stays a browser API rather
// than a new native audio-capture dependency (e.g. `cpal`) -- `getUserMedia`/
// `MediaRecorder` are standard, already permission-gated by the WebView,
// and produce compressed output (usually webm/opus in a Chromium-based
// WebView2) without this crate needing to touch a raw audio device at all.
// ffmpeg (already bundled) decodes whatever container the browser actually
// produced, so the frontend doesn't need to police a specific mimeType.

use tokio::process::Command;

use crate::util::{cli_path, unique_temp_path};

/// One filter chain, one ffmpeg process, for both format conversion and
/// cleanup -- verified directly against a real synthesized-speech clip with
/// 2s of near-silence prepended (not just read off ffmpeg's docs): the
/// trimmed output kept the full sentence intact (nothing at the start got
/// eaten along with the silence) and still transcribed word-for-word
/// afterward.
///
/// Order matters: `highpass` (cut sub-80Hz rumble/handling noise) and
/// `afftdn` (FFT noise reduction) run *before* `silenceremove`, so the
/// silence-detection threshold is judging already-cleaned audio, not raw
/// noise that could otherwise register as "still speech" and defeat the
/// trim. `silenceremove`'s `start_periods=1` deliberately only ever touches
/// the leading silence -- it does not also chew on brief pauses between
/// words/sentences later in the clip, which would make speech sound
/// unnaturally clipped together.
///
/// `loudnorm` runs single-pass here (not the two-pass measure-then-apply
/// `loudness.rs` uses for a final exported video) -- "sounds noticeably
/// clearer" is the actual goal for a short dictation/voice-over clip, not
/// broadcast-spec loudness compliance, so the extra ffmpeg round-trip a
/// measure pass would cost isn't worth paying here. Loudnorm resamples
/// internally (confirmed directly: an unconstrained loudnorm pass on a
/// 16kHz input came out at 192kHz), which is exactly why `-ar 16000 -ac 1`
/// stay on the *output* side of this same command rather than trusting the
/// filter to preserve the input format.
const ENHANCE_FILTER: &str =
    "highpass=f=80,afftdn=nf=-25,silenceremove=start_periods=1:start_duration=0.1:start_threshold=-40dB:detection=peak,loudnorm=I=-16:TP=-1.5:LRA=11";

#[tauri::command]
pub async fn save_recorded_voice(bytes: Vec<u8>) -> Result<String, String> {
    let raw_path = unique_temp_path("mic-recording-raw", "webm");
    std::fs::write(&raw_path, &bytes).map_err(|e| format!("Couldn't save the recording: {e}"))?;

    let wav_path = unique_temp_path("mic-recording", "wav");
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y",
            "-i",
            &cli_path(&raw_path),
            "-af",
            ENHANCE_FILTER,
            "-ar",
            "16000",
            "-ac",
            "1",
            "-acodec",
            "pcm_s16le",
        ])
        .arg(cli_path(&wav_path))
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg to convert the recording: {e}"))?;

    let _ = std::fs::remove_file(&raw_path);

    if !output.status.success() {
        let _ = std::fs::remove_file(&wav_path);
        return Err(format!(
            "Couldn't convert the recording to a usable audio file: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(cli_path(&wav_path))
}
