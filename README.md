# Reels Caption App

A local-first **Tauri (Rust) + React** app that captions video: pick a
video → extract audio + transcribe & align with word-level timestamps
(`ffmpeg` + [NVIDIA Parakeet](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2)
for English, a dedicated Indic Whisper fine-tune per language for Indian
languages) → style the captions → burn them back onto the video
(`ffmpeg` + libass). Also runs a local LLM service (llama.cpp +
Qwen2.5-0.5B-Instruct) for title/description/hashtag/emoji generation.
Targets macOS, Windows, Linux, iOS, and Android from one codebase.

The app itself is gated behind a Firebase Auth login/signup screen (see
section 4) — the one deliberate exception to the "local-first, no account
needed" design everything else here follows.

Transcription has gone through three prior engines: whisper.cpp + Montreal
Forced Aligner (MFA), then WhisperX (faster-whisper + wav2vec2 forced
alignment in one tool). This is the current iteration, driven by wanting
genuinely Indian-language-tuned models instead of a generic multilingual
one: **Parakeet** (CC-BY-4.0, NVIDIA's fastest-on-CPU English ASR, via the
`onnx-asr` package — no PyTorch needed at all for English) for English,
and a per-language Indic Whisper fine-tune (Tamil currently uses
[vasista22/whisper-tamil-small](https://huggingface.co/vasista22/whisper-tamil-small),
chosen over AI4Bharat's larger IndicWhisper checkpoints for memory reasons
— see section 2.2) for Indian languages. Word timestamps for both come
from each model's own native decoder output, not a separate
forced-alignment step — an earlier version of this pipeline reused
WhisperX's `align()` function against a wav2vec2 CTC model, but that
turned out to fall apart on long, un-punctuated segments (see section
2.2's full writeup). Sarvam STT was considered and rejected: it's a cloud
API with no self-hostable model, which would have meant every clip
leaving the machine — a reversal of this app's local-first design. See
`src-tauri/src/stt.rs`'s module doc comment for the full story.

## What's in the box

```
reels-caption-app/
├── src/                             # React frontend
│   ├── components/
│   │   ├── auth/                    # AuthScreen.jsx -- login/signup, gates the whole app (section 4)
│   │   ├── shell/                   # AppShell/MenuBar/Sidebar/MainPanel/TranscriptPanel/Timeline/ToolCard/VoiceoverSection/
│   │   │                            #   TranscribeStatus/BurnExportButton/MoreOptionsModal/DeveloperToolsPanel
│   │   ├── tools/                   # panels slotted into ToolCard/MoreOptionsModal: VoSync, Prosody, Diarize, Loudness, Ducking, Keywords, ContentIdeas
│   │   ├── CaptionStyleEditor.jsx   # font/color/position/animation controls + live preview
│   │   ├── TranscriptEditor.jsx     # editable word-level transcript, used by shell/TranscriptPanel.jsx
│   │   ├── VideoPreview.jsx         # video element + scrub controls + voiceover sync, wrapped by shell/MainPanel.jsx
│   │   └── SilenceRemovalPanel.jsx
│   ├── context/
│   │   └── AuthContext.jsx          # Firebase Auth session state (see section 4)
│   ├── lib/                         # pure-JS helpers: captions.js, keywords.js (RAKE-lite hashtags), themes.js, time.js
│   ├── firebase.js                  # Firebase app init, reads VITE_FIREBASE_* env vars (see section 4)
│   ├── main.jsx
│   ├── App.jsx                      # owns cross-cutting state, renders <AuthScreen /> or <AppShell />
│   └── styles.css
├── src-tauri/                       # Rust backend (Tauri)
│   ├── src/
│   │   ├── lib.rs                   # Tauri command registration + greet/check_ffmpeg
│   │   ├── pipeline.rs              # extract audio (ffmpeg) + transcribe (stt.rs) -> word timestamps; transcribe_audio_file for a voiceover's own WAV
│   │   ├── stt.rs                   # Parakeet (English) + Indic Whisper fine-tunes (Indian languages) conda-env resolution
│   │   ├── tts.rs                   # Piper (English) + MMS-TTS (Indian languages) voiceover generation, emotion-preset delivery
│   │   ├── llm.rs                   # local LLM service: llama-server.exe running Qwen2.5-0.5B-Instruct
│   │   ├── content_ideas.rs         # title/description/hook/hashtags/emoji generation on top of llm.rs
│   │   ├── slang.rs                 # optional Tanglish slang spelling normalization
│   │   ├── conda_util.rs            # shared conda-environment discovery (stt, media-ai envs)
│   │   ├── captions.rs              # ASS subtitle generation + burn-in via ffmpeg's `ass` filter, prosody emphasis
│   │   ├── segments.rs              # splits long burn jobs into parallel ffmpeg chunks + lossless concat
│   │   ├── jumpcuts.rs              # silence/filler-word removal ("jump cuts") from word timestamps
│   │   ├── diarize.rs               # lightweight speaker diarization (silence-gap segments + media-ai MFCC clustering)
│   │   ├── loudness.rs              # two-pass ffmpeg `loudnorm` audio normalization
│   │   ├── ducking.rs               # auto background-music ducking under speech, pure ffmpeg
│   │   ├── vosync.rs                # voice-over <-> mouth-movement cadence sync (standalone tool + offset-only mode for live preview)
│   │   ├── media_ai.rs              # shared plumbing for the "media-ai" conda env (vosync/prosody/diarize scripts)
│   │   ├── ffmpeg.rs                # shared async ffmpeg invocation + live progress reporting
│   │   ├── bin_paths.rs             # resolves bundled/PATH ffmpeg, ffprobe, and fonts dir
│   │   ├── proc_cleanup.rs          # kills child ffmpeg/stt/media-ai processes when the app exits
│   │   ├── util.rs                  # shared temp-file + transcript-freshness helpers
│   │   └── main.rs                  # desktop entry point
│   ├── stt/                         # Python scripts stt.rs shells out to (parakeet_transcribe.py, indic_transcribe.py)
│   ├── tts/                         # Python scripts tts.rs shells out to (piper_synthesize.py, mms_synthesize.py)
│   ├── media_ai/                    # Python scripts media_ai.rs shells out to (vo_sync.py, prosody.py, diarize.py)
│   ├── resources/bin/               # drop ffmpeg.exe/ffprobe.exe here (see section 2)
│   ├── resources/llama/             # llama-server.exe + qwen2.5-0.5b-instruct-*.gguf (see the LLM service section)
│   ├── resources/stt-models/        # converted Indic Whisper checkpoints, one dir per language
│   ├── resources/tts-models/        # bundled Piper English voice (see section 2.6)
│   ├── resources/fonts/             # bundled Noto Sans Tamil (see section 2.5)
│   ├── capabilities/default.json
│   ├── Cargo.toml
│   ├── build.rs
│   └── tauri.conf.json
├── index.html
├── package.json
└── vite.config.js
```

## 1. Install prerequisites (once, on your dev machine)

- **Node.js** 18+ — https://nodejs.org
- **Rust** — https://rustup.rs (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **Tauri CLI** — installed via npm below, no separate install needed
- **Platform-specific Tauri prerequisites** — follow the official checklist for your OS: https://v2.tauri.app/start/prerequisites/
  - macOS: Xcode Command Line Tools
  - Windows: Microsoft C++ Build Tools + WebView2 (usually preinstalled on Win 11)
  - Linux: `webkit2gtk`, `libayatana-appindicator3-dev`, etc. (see the link above for your distro)

**Windows without Visual Studio / C++ Build Tools:** it's possible to build
with a plain MinGW-w64 toolchain (`rustup toolchain install
stable-x86_64-pc-windows-gnu`, then `rustup default
stable-x86_64-pc-windows-gnu`) instead of the multi-GB Build Tools install.
One thing to know if you go this route: rustc's `cdylib` output on
`-windows-gnu` auto-exports every public symbol, and Tauri's dependency
tree produces more than the PE format's 65,535-export limit — a build will
fail with `error: too many exported symbols`. This doesn't affect MSVC or
macOS/Linux. `src-tauri/Cargo.toml`'s `[lib] crate-type` is currently
narrowed to `["rlib"]` to work around it (see the comment there); restore
`["staticlib", "cdylib", "rlib"]` before cross-compiling for iOS/Android,
or if you switch to MSVC.

## 2. Install ffmpeg and the STT engines locally (desktop only, for now)

**ffmpeg**
- macOS: `brew install ffmpeg`
- Windows: `winget install ffmpeg` (or download from ffmpeg.org and add to PATH)
- Linux: `sudo apt install ffmpeg`

**The `stt` conda env** (transcription + alignment for both languages).
Needs conda (not plain pip — installs native/CUDA-capable dependency
wheels pip alone doesn't resolve as cleanly). Install
[Miniconda](https://www.anaconda.com/download) if you don't already have
`conda`/`mamba` set up, then:

```bash
conda create -n stt python=3.10 -y
conda run -n stt pip install faster-whisper "onnx-asr[cpu,hub]" soundfile
```

`src-tauri/src/stt.rs` looks for `conda` on PATH, then common install
locations (`%ProgramData%\miniconda3`, `%USERPROFILE%\miniconda3`, and the
same under every other fixed drive letter, e.g. `D:\miniconda3`) — for
each one it finds, it actually runs
`python -c "import onnx_asr, faster_whisper"` inside the `stt`
env to confirm that specific install has it working correctly. If none of
that finds it, set `REELS_CAPTION_APP_STT_CONDA_PATH` to your
`conda`/`conda.exe` path directly. Unlike a typical `conda run`
invocation, real calls resolve the env's root directory once and then
invoke `python.exe` inside it *directly* — a bug in this project's conda
version (confirmed empirically) crashes `conda run` outright on certain
argument shapes, e.g. a Hugging Face model id containing a `/`.

### 2.1. English: NVIDIA Parakeet

Runs through [`onnx-asr`](https://github.com/istupakov/onnx-asr) — pure
ONNX Runtime, no PyTorch needed for this half of the pipeline at all,
which is also why it's noticeably faster to start than the Indic path.
The model (`nemo-parakeet-tdt-0.6b-v2`, CC-BY-4.0) downloads and caches
itself automatically on first use via `huggingface-hub` (a few GB —
budget time for the first real transcription).

Parakeet's own output is per-token timestamps, not word-level —
`src-tauri/stt/parakeet_transcribe.py` reconstructs words from the
token/timestamp pairs the same way this project has always merged
sub-word ASR tokens into words (a token with a leading space starts a new
word; a word's end is approximated as the next word's start, or clip
duration for the last word).

### 2.2. Indian languages: Indic Whisper fine-tunes

Generic multilingual Whisper underperforms on Indic content, so each
language uses a dedicated fine-tune instead — converted once to
CTranslate2 (int8) via `ct2-transformers-converter`, since none of these
ship as a ready-to-run CTranslate2/ONNX model. See
`src-tauri/resources/stt-models/README.md` for the exact steps for both
sources below; only Tamil is wired up today (`stt::INDIC_LANGUAGES`).

Two sources, picked per-language based on how much RAM the converting
machine has free (the converter has to fully materialize the checkpoint in
memory before quantizing):
- [AI4Bharat's IndicWhisper](https://github.com/AI4Bharat/vistaar)
  (MIT-licensed, whisper-medium fine-tunes, ~1.4GB, more accurate, a plain
  HTTPS zip per language, no account/gating) — the original plan, but its
  checkpoint repeatedly ran out of memory converting on this project's dev
  machine (which had only 0.3–2GB free RAM at the time).
- **Tamil uses this instead:**
  [vasista22/whisper-tamil-small](https://huggingface.co/vasista22/whisper-tamil-small)
  (IIT Madras, Apache 2.0, whisper-small fine-tune, 244M params — ~1/3 the
  size), the same model this project used successfully in its earlier
  whisper.cpp era. Verified end-to-end on real Tamil audio: clean,
  accurate transcript, tight word-level timestamps (see below).

Word timestamps come straight from `faster-whisper`'s own native decoder
output (`word_timestamps=True` in `src-tauri/stt/indic_transcribe.py`), not
a separate forced-alignment step. An earlier version of this pipeline
forced-aligned against a wav2vec2 CTC model
([Harveenchadha/vakyansh-wav2vec2-tamil-tam-250](https://huggingface.co/Harveenchadha/vakyansh-wav2vec2-tamil-tam-250),
reused via WhisperX's `align()` function) on the theory that Whisper's
native cross-attention timestamps drift on long-form audio — a real,
confirmed problem this project hit before, on a different model. That
looked fine on a first pass (monotonically increasing timestamps), but
real usage on Tamil audio surfaced the actual bug: `faster-whisper` was
producing occasional huge (20-30s), unsplit segments for this model, and
forced-aligning a giant block of un-punctuated text against that much
dense speech is exactly where CTC alignment falls apart — both the
misaligned word-highlighting and the "missing" words users saw (never
dropped by the ASR; they just failed to get a placeable timestamp during
alignment and were silently filtered out downstream).

The real fix is capping how much audio `faster-whisper`'s decoder ever
looks at in one internal pass — the `chunk_length` argument to
`model.transcribe()`, **not** `vad_parameters`/`vad_filter`, which only
controls what survives the pre-decode silence-stripping pass and has zero
effect on segment length (verified directly: identical output with
`vad_filter` on or off). Capped at 15s, every word timestamp across a real
60-second Tamil clip landed under 1.1s, versus an 11-second single-word
artifact in the uncapped version. This also means WhisperX and NLTK are no
longer dependencies anywhere in this pipeline.

**Why not Sarvam STT:** Sarvam AI's Saarika/Saaras speech-to-text models
are a cloud REST API only — there is no self-hostable checkpoint, unlike
their open-sourced LLMs (Sarvam-1, Sarvam-M, ...). Routing audio through
it would mean every clip leaves the machine and gets billed per request,
which is the same tradeoff a cloud-LLM proposal was rejected for
elsewhere in this project. IndicWhisper was chosen instead specifically
to stay fully local.

### 2.3. Local LLM service: llama.cpp + Qwen2.5-0.5B-Instruct (title/description/hashtags/emoji)

The only part of this app's ML stack that runs as a long-lived local HTTP
server rather than a one-shot subprocess — reloading a model on every
request would make even one suggestion take as long as the model load
itself. `llm.rs` starts `llama-server.exe` lazily on first use and reuses
it for the app's lifetime.

```bash
# 1. llama.cpp: official prebuilt CPU-only Windows binary, no CUDA/GPU
#    needed (matches this project's CPU-first design elsewhere) — grab
#    "llama-b<version>-bin-win-cpu-x64.zip" from
#    https://github.com/ggml-org/llama.cpp/releases/latest and extract it
#    into src-tauri/resources/llama/

# 2. Qwen2.5-0.5B-Instruct, Q4_K_M GGUF quant (~490MB), Qwen's own
#    official quant:
curl -L -o src-tauri/resources/llama/qwen2.5-0.5b-instruct-q4_k_m.gguf \
  https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf
```

This app previously used Sarvam-1 (2B) here — an Indic-branded model, but
a base/text-completion model, not instruction-tuned, which meant every
feature built on it needed a hand-rolled few-shot prompt and fragile
line-based text parsing. That became a real liability once the feature
grew from a single hook line into five structured fields (title,
description, hook, hashtags, emoji): Qwen2.5-0.5B-Instruct is genuinely
instruction-tuned, so `content_ideas.rs` just asks for the fields it
wants directly, and `llm::complete`'s `json_schema` parameter constrains
`llama-server`'s output to valid JSON matching an exact schema — no more
parsing free-form text at all. It's also noticeably lighter (~490MB vs.
~1.5GB) and faster to load (under 2 seconds on this project's dev
machine) than Sarvam-1 was, which matters more on a RAM-constrained
laptop than the Indic branding did in practice, since this feature is
content-generation *about* a transcript, not translation or ASR.

One real quality gap found during testing: a 0.5B model, even
instruction-tuned, doesn't reliably know what "emoji" means on its
own — asked plainly, it returned emoji *names* as plain text (`.sleep`,
`calming`) instead of actual Unicode pictograph characters. Fixing this
took giving it concrete example codepoints directly in the system prompt
(see `SYSTEM_PROMPT` in `content_ideas.rs`) rather than trusting the word
"emoji" alone — after that, real emoji came back consistently. A separate,
milder issue (an occasional stray space inside a generated hashtag) gets
cleaned up in Rust rather than trusted from the model — see
`sanitize_hashtag` in the same file.

## 2.4. Install the media-ai env (optional, voice-over sync / vocal emphasis / speaker diarization)

These three features share one conda env, `media-ai` — mediapipe (lip
landmarks), librosa (audio feature extraction), and scipy (signal
correlation / clustering math), deliberately **not** PyTorch or any
gated/account-requiring model:

```bash
conda create -n media-ai python=3.11 -y
conda run -n media-ai pip install -qU "mediapipe==0.10.9" librosa scipy numpy
```

`mediapipe` is pinned to `0.10.9` deliberately — newer releases (confirmed against 0.10.35) dropped the legacy `solutions` API (`mp.solutions.face_mesh`) this project's voice-over sync tool uses, keeping only the newer Tasks API (which needs a separately-downloaded `.task` model file). Pinning avoids that extra download step.

`src-tauri/src/media_ai.rs` looks for a conda install with this `media-ai`
env the same way `stt.rs` looks for `stt` (see section 2 above, shared
discovery logic lives in `conda_util.rs`) — every fixed drive letter,
verified for real rather than assumed. Same override pattern applies via
`REELS_CAPTION_APP_MEDIA_AI_CONDA_PATH` if none of that finds it.
Dev-machine-only for now (no bundled/zero-install path), same as the STT
engines' system-conda dependency.

Unlike the (removed) vision-LLM feature, each media-ai script here is a
fast one-shot CPU operation — no persistent worker process, no
multi-second model load to amortize.

## 2.5. Bundled fonts (for scripts your system fonts might not render correctly)

`src-tauri/resources/fonts/` ships **Noto Sans Tamil** (Google's
open-source, SIL-licensed Tamil font — full Unicode coverage, GSUB/GPOS
tables for correct Indic shaping) so Tamil captions render properly
(conjuncts/vowel signs correctly combined with their base consonant)
regardless of what's installed system-wide. Pick "Noto Sans Tamil" in the
caption style editor's font dropdown for Tamil content.

Mechanism: `src-tauri/src/bin_paths.rs`'s `fonts_dir()` resolves this
directory the same 3-tier way as ffmpeg (env var override → bundled
resource → this crate's own `resources/fonts/` as the dev-machine
fallback), and every `ass` burn filter (`captions.rs`, `segments.rs`)
passes it via ffmpeg's `fontsdir` option — this tells libass to look
there in addition to system fonts, without needing anything actually
*installed*.

To add another script's font later (e.g. Devanagari for Hindi): drop a
`.ttf`/`.otf` into `resources/fonts/`, add its exact font-family name
(check with a font inspector, e.g. Python's `fontTools`) to
`FONT_OPTIONS` in `src/components/CaptionStyleEditor.jsx`, and put any
license file in `resources/fonts-license/` instead of `resources/fonts/`
— libass scans every file in `fontsdir` and will (harmlessly, but
noisily) try to load non-font files it finds there too.

**This only fixes the video-burn output — the app's own UI (transcript
editor, etc.) is a separate rendering path (the WebView, not
ffmpeg/libass) and needed its own fix.** A second copy of the same font
lives at `src/assets/fonts/` (Vite-bundled as a static web asset) and is
registered via `@font-face` + added as a fallback in `styles.css`'s base
`font-family` stack, so Tamil text anywhere in the UI renders correctly
through ordinary per-character browser font-fallback — no per-component
language detection needed. One easy-to-miss gotcha this relied on:
browsers don't inherit `font-family` into form controls (`<input>`,
`<select>`, etc.) from `body`/`:root` by default, so `styles.css` also
has an explicit `font-family: inherit` reset for those elements —
without it, the Tamil fallback would silently never reach the transcript
editor's word `<input>` boxes specifically.

## 2.6. Text-to-speech voiceover: Piper (English) + MMS-TTS (Indian languages)

Same "small, dedicated, CPU-friendly model per language" shape as
transcription (section 2 above), for the same reasons — both are
lightweight VITS-family models. A separate conda env, `tts`, since neither
model shares dependencies with `stt` or `media-ai`:

```bash
conda create -n tts python=3.10 -y
conda run -n tts pip install piper-tts
conda run -n tts pip install torch --index-url https://download.pytorch.org/whl/cpu
conda run -n tts pip install transformers scipy
```

**English (Piper):** a ~63MB ONNX voice
([rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices), `en_US-lessac-medium`),
small enough to bundle like the fonts/llama.cpp resources rather than leave
to per-machine setup — drop it at
`src-tauri/resources/tts-models/en/en_US-lessac-medium.onnx` (+ its
`.onnx.json`):
```bash
curl -L -o src-tauri/resources/tts-models/en/en_US-lessac-medium.onnx \
  https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx
curl -L -o src-tauri/resources/tts-models/en/en_US-lessac-medium.onnx.json \
  https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json
```
Verified directly: a warm process synthesizes close to real-time, and
round-tripping the output back through Parakeet (this app's own English
ASR) produced an exact word-for-word match of the input script — real,
intelligible speech, not just valid-looking audio bytes. A *fresh* process
pays a one-time ~10s ONNX Runtime graph-optimization cost on its first
call, though — since `tts.rs` runs each request as its own subprocess
(same pattern as `stt.rs`), every voiceover generation pays this, not just
the first one. Acceptable for a "generate a file" feature, same tradeoff
already accepted for transcription/caption-burning elsewhere in this app.

**Indian languages (MMS-TTS):** Meta's per-language VITS checkpoints
(`facebook/mms-tts-<code>`, keyed by ISO 639-3 — see
`mms_synthesize.py`'s `LANGUAGE_TO_MMS_CODE`), downloaded on demand via
`transformers` and cached under `~/.cache/huggingface` — no manual
conversion step needed, unlike `resources/stt-models/`. Only Tamil is
wired up today (`tts::TTS_INDIC_LANGUAGES`, independent from
`stt::INDIC_LANGUAGES` — a checkpoint existing for transcription says
nothing about synthesis, and vice versa). Verified directly: real Tamil
audio, correct length, but meaningfully slower than Piper — consistently
~3x slower than real-time, with no warm-up speedup the way Piper has.
Still fine for generating a voiceover file. A community-converted ONNX
export of MMS-TTS exists and would likely be faster, but this project uses
Meta's own officially-published checkpoint instead — the same reasoning
that favored an official GGUF over a third-party quant for the LLM service
(section 2.3).

**Live in-preview sync, not a separate merged file:** generating a
voiceover with a video loaded automatically calls
`vosync::compute_voiceover_offset` (the same mouth-movement correlation
`sync_voice_over`/`vosync.rs` uses for its standalone tool, via
`vo_sync.py --offset-only` — same algorithm, just skipping the ffmpeg mux
step) to find a timing offset, then plays the voiceover directly alongside
the loaded video in the main preview (`VideoPreview.jsx`): a hidden
`<audio>` element kept in lockstep with the `<video>` element's own
play/pause/seek events, offset-corrected, with the video's own audio
muted while a voiceover is active. No new file gets written just to
preview this — deliberately, per how this feature is meant to be used
(type a script, hear it against the video immediately, iterate). This
replaced an earlier version of this feature with separate "Save as…",
"Merge with video…", and "Use in Voice-over Sync…" hand-off buttons
(`voiceover_mux.rs`, now removed) — worth knowing if you're looking for
those in history; they added real steps between typing a script and
hearing it against the video, which is the opposite of what this feature
is for. If a face isn't detected (e.g. B-roll with no visible speaker),
sync fails gracefully — the voiceover still plays, just without offset
correction, and the error is shown inline rather than blocking playback.
`sync_voice_over`/`vosync.rs`'s standalone tool (a separate sidebar entry,
**Voice-over Sync**) still exists unchanged for muxing an *externally
recorded* voice-over onto a video into an actual output file — that's a
genuinely different job (a real exported file, not a preview) from
what generating a voiceover in-app needs.

Two real bugs found and fixed via direct testing, not just code review:
1. **Offset sign was inverted.** `vo_sync.py`'s own doc comment is explicit
   ("positive = delay the voice-over, negative = advance it"), and its
   `mux()` applies that via ffmpeg's `-itsoffset <offset>` on the voiceover
   input — meaning the voiceover position that belongs at video time `t` is
   `t - offset`, not `t + offset`. `VideoPreview.jsx`'s sync function had
   the `+` version live for one turn; it produced exactly the symptom that
   math predicts (a negative offset read as "hasn't started yet," so the
   voiceover appeared to wait until partway through the video before
   playing at all) before being caught and fixed.
2. **A negative ("advance") offset skips real content, not just timing.**
   With the sign now correct, a negative offset makes the sync function
   jump `audio.currentTime` straight to `|offset|` seconds in at
   video-start — confirmed directly (round-tripped a generated file
   through this app's own Tamil ASR) that the underlying audio always
   starts at 0:00 with the full script intact, so this jump *skips real
   generated content* rather than just being imperfectly timed. Since an
   unrelated video's mouth movement correlated against freshly generated
   narration has no genuine timing relationship to find, a spurious
   negative offset is a realistic outcome here, not a rare edge case — so
   `syncVoiceoverToVideo` clamps the offset to `Math.max(0, offset)`
   before use: this preview only ever *delays* playback start, never
   truncates the beginning. `vo_sync.py`'s actual mux (a genuinely
   advance-able external recording) is unaffected — the clamp is local to
   the live-preview path only.
3. **Burn & Export never actually used the voiceover.** The live-preview
   sync only ever fed `VideoPreview.jsx`'s playback — `burn_captions`
   (`captions.rs`) had no `voiceover_path` parameter at all, so exporting
   always burned the *original* video's own transcript over the
   *original* audio, silently ignoring any active voiceover entirely, a
   real reported bug. Two things were missing, not one: (a)
   `burn_captions` gained optional `voiceover_path`/
   `voiceover_offset_seconds` params — when set, ffmpeg gets a second
   `-itsoffset`'d input mapped as the audio stream (same convention as
   `vo_sync.py`'s mux) instead of `-c:a copy`-ing the original; verified
   directly that this composes correctly with the existing `ass=...`
   caption-burn filter in one pass. (b) burning still needs real
   word-level timestamps for whatever the voiceover actually says, not
   the original video's words — `pipeline::transcribe_audio_file` (a
   thin wrapper skipping `run_pipeline`'s ffmpeg-extraction step, since a
   generated/uploaded voiceover is already a WAV) re-transcribes it right
   after sync, and `App.jsx`'s `handleVoiceoverReady` adopts those words
   (offset-shifted to match the video's timeline) as the app's working
   transcript — what the Transcript panel, Timeline, and Burn & Export
   all now use. Stale `prosody`/`speakers` data (computed against the old
   transcript's word count and timing) gets cleared at the same time,
   same reasoning. A voiceover also forces `burn_captions` off the
   segmented-parallel-encode path (`segments.rs` has no notion of a
   second audio file/offset to slice in lockstep) — correctness over the
   speedup, and typical reels are short enough that this rarely matters.

**Emotion-driven delivery (opt-in checkbox):** classifies the script's
tone via the LLM service (section 2.3) into one of five presets (neutral,
excited, somber, urgent, calm), then applies a modest rate + pitch
adjustment — rate via each engine's own native synthesis parameter (Piper's
`length_scale`, MMS-TTS's `speaking_rate` — note the *opposite* conventions,
handled in `tts.rs`'s `emotion_preset`), pitch via a post-process step
using ffmpeg's `rubberband` filter (formant-preserving, confirmed present
in this project's bundled ffmpeg build). Not real emotional performance
and not driven by a speaker's visible movement — see the panel's own hint
text and the "possibilities" discussion this feature came out of.

A plain zero-shot instruction ("classify into one of: ...") was verified
directly to fail badly at this model size: a script literally saying "We
just hit one million subscribers!" came back "calm", and every test
script defaulted toward "calm" regardless of content — at both
temperature 0.7 and temperature 0, so not a sampling-randomness issue, a
genuine capability gap at 0.5B params for 5-way classification. Concrete
worked examples baked into the system prompt (few-shot, not zero-shot)
fixed it — verified directly, 4/5 correct on a fresh test set afterward.
`llm::complete` also gained a `temperature` parameter as part of this
(low for this single-best-label classification call, unlike the higher
temperature `content_ideas.rs`'s creative-generation call wants) — worth
knowing if you touch either prompt: don't strip the few-shot examples back
down to a bare instruction without re-verifying against real scripts.

## 2.7. Voice cloning: matching the generated voiceover to the original speaker

Generating a voiceover from typed text ("write voiceover text" mode) doesn't just
synthesize *a* voice anymore — it tries to make the result actually sound like the
person speaking in the loaded video. This layers on top of section 2.6's synthesis
rather than replacing it: Piper/MMS-TTS still generate the actual speech from the
script text, and a separate reshaping step then matches its *timbre* to the original
speaker.

**[OpenVoice V2](https://github.com/myshell-ai/OpenVoice)** (MIT license, free for
commercial use since April 2024) does the reshaping via its `ToneColorConverter` — it
operates on acoustic features only, not linguistic content, so it works the same way
regardless of the synthesized language. This was the deciding factor over a
cloning-native TTS model like XTTS: XTTS doesn't support Tamil at all, which would have
broken this app's Indic-language story; layering a language-agnostic converter on top
of the existing per-language engines doesn't have that problem.

Its own conda env, `voice-clone` — not folded into `tts` or `media-ai`. OpenVoice's own
`requirements.txt` pins old dependency versions (`faster-whisper==0.9.0`,
`librosa==0.9.1`, `numpy==1.22.0`) that risk conflicting with the `stt` env's newer
`faster-whisper` (already required for `detect_language.py`, section 7) or the
`media-ai` env's `librosa`, and drags in Chinese/Japanese text-normalization packages
and `gradio` this app has no use for. Isolating it avoids any version-conflict risk to
the working envs.

**A plain `pip install git+...` fails outright — verified directly.** OpenVoice's pinned
`faster-whisper==0.9.0` pulls in `av==10.*`, which has no prebuilt wheel for a modern
Python/Windows combination; pip falls back to building it from source, which fails
("Getting requirements to build wheel: finished with status 'error'"). The fix,
confirmed working end-to-end: install OpenVoice with `--no-deps` (skips its pinned
`requirements.txt` entirely) and install its actual runtime dependencies separately with
modern versions instead — a current `faster-whisper` pulls a current `av` release that
*does* have a prebuilt wheel, sidestepping the problem completely:
```bash
conda create -n voice-clone python=3.10 -y
conda run -n voice-clone pip install torch --index-url https://download.pytorch.org/whl/cpu
conda run -n voice-clone pip install --no-deps git+https://github.com/myshell-ai/OpenVoice.git
conda run -n voice-clone pip install soundfile librosa inflect unidecode eng_to_ipa pypinyin cn2an jieba pydub faster-whisper whisper-timestamped wavmark
```
Then download the OpenVoice V2 converter checkpoint (`config.json` + `checkpoint.pth`,
~125MB) into `src-tauri/resources/voice-clone-models/converter/` — directly from the
[OpenVoiceV2 model repo](https://huggingface.co/myshell-ai/OpenVoiceV2)'s `converter/`
subfolder (that's the only piece needed; the repo's `base_speakers/` checkpoints are for
OpenVoice's own MeloTTS voices, unused here since Piper/MMS-TTS already do synthesis).
Not bundled with the app — same per-machine setup treatment as the `stt`/`media-ai`
models.

**The first real cloning call may prompt once to trust a `torch.hub` repo — answer
yes.** `se_extractor.get_se()`'s VAD segmentation loads Silero VAD via
`torch.hub.load('snakers4/silero-vad', ...)`, which asks an interactive
"Do you trust this repository? (y/N)" the first time any repo is loaded this way on a
machine. Answering yes caches the decision in `~/.cache/torch/hub/trusted_list` —
confirmed directly that a second call, even with stdin closed (matching exactly how
`voice_clone.rs` spawns the subprocess), then succeeds silently with no prompt. This is
a genuinely one-time, per-machine step, not something to script around — set up the env,
run one manual cloning attempt from a terminal to answer the prompt, and every call from
the app afterward (including a closed-stdin subprocess) just works.

**A real cross-environment gotcha found on this project's own dev machine, worth
knowing if you hit a mysterious `ModuleNotFoundError` in an unrelated conda env right
after setting up `voice-clone`:** some of this machine's conda environments (`stt`,
`tts`) turned out to be silently relying on a Python-version-wide *shared* per-user
site-packages directory (`%APPDATA%\Python\Python3<x>\site-packages`) for a handful of
common packages like `typing_extensions`, rather than having their own copies —
apparently pre-existing on this machine, not introduced by this project's env setup.
Installing/upgrading packages into `voice-clone` (a same-Python-version env) uninstalled
and replaced a package in that shared location, and both `stt` and `tts` broke with
`ModuleNotFoundError: No module named 'typing_extensions'` until it was reinstalled
directly *into* each affected env (`<env>/python.exe -m pip install typing_extensions`),
making them self-contained instead of relying on the shared location. If any other env
starts failing the same way after a `voice-clone` setup/update, that's almost certainly
this — reinstall whatever's missing directly into the broken env, not into `voice-clone`.

**Decision order** (`tts::resolve_voice_reference` + `generate_voiceover`), always
attempted automatically — no toggle, matching this app's "no dropdown, auto-detect and
degrade gracefully" approach already established for language detection (section 7):
1. The loaded video has an audio track (`ffmpeg::has_audio_stream`) → pull a bounded
   (~30s) reference clip of its own audio (`voice_clone::extract_reference_clip`).
2. Detect the reference speaker's rough gender from that clip — median pitch via
   `librosa.pyin` in the existing `media-ai` env (`media_ai/detect_gender.py`), a fast
   step that runs regardless of whether cloning ultimately succeeds, so the fallback
   tier below is ready either way.
3. Synthesize the script as usual (section 2.6) — for English, picking the
   gender-matched Piper voice up front (`en_US-hfc_male-medium` if the reference
   sounded male, `en_US-lessac-medium` otherwise). **Tamil has no second voice to pick
   from** — `facebook/mms-tts-tam` is a single-speaker checkpoint, a real constraint
   confirmed directly via `mms_synthesize.py` (it only ever loads one checkpoint per
   language), not a gap this app's code introduces. Tamil's fallback tier is simply
   today's one default voice, unchanged.
4. Attempt real cloning (`voice_clone::clone_voice`) on that synthesis. On success,
   that's the final audio — genuinely reshaped toward the original speaker's timbre.
5. On any failure (no reference audio, cloning engine/checkpoint missing, the
   conversion itself erroring) — the step-3 synthesis is kept as-is (already
   gender-matched for English where possible), and a clear, non-blocking note explains
   why real cloning wasn't used. Voiceover generation never fails outright just because
   cloning couldn't run.
6. The existing emotion pitch-shift (section 2.6) still applies last, unchanged.

`VoiceoverResult` (the `generate_voiceover` command's return type) reports which tier
actually produced the audio — `voice_match: "cloned" | "gender_matched" | "default"`,
plus an optional `voice_match_note` explaining a fallback — surfaced in
`VoiceoverSection.jsx` as "Voice matched to the original speaker" / "Closest voice
match" / "Default voice", the same transparency precedent the existing emotion badge
already sets.

**Bundled male English Piper voice:** a second Piper voice,
`en_US-hfc_male-medium` (~63MB, same [rhasspy/piper-voices](https://huggingface.co/rhasspy/piper-voices)
source as `en_US-lessac-medium`), lives at
`src-tauri/resources/tts-models/en/en_US-hfc_male-medium.onnx` (+ its `.onnx.json`) —
bundled the same way as the existing default voice, purely so the gender-matched
fallback tier has a real second option to pick from.

This only applies to "write voiceover text" mode. "Upload voice-over" is the user's own
external recording — cloning it onto anything would be nonsensical, so that path
(`pickAndUpload` in `VoiceoverSection.jsx`) never passes a video path through and always
skips straight to re-transcription.

**Verified directly, not just import-checked:** cloned a Piper female voice
(`en_US-lessac-medium`, median pitch 204Hz) onto a Piper male reference
(`en_US-hfc_male-medium`, median pitch 113Hz) via `clone_voice.py` standalone. The
cloned output's own median pitch came back at 122Hz — clearly reshaped toward the
reference, not left at the source's 204Hz — and `detect_gender.py` correctly read the
source as female, the reference as male, and the cloned output as male too. Round-tripped
the cloned output back through this app's own English ASR (`parakeet_transcribe.py`):
an exact word-for-word match of the original script, confirming intelligibility survives
the conversion, same verification precedent as Piper's own plain output (section 2.6).

## 3. Install project dependencies

```bash
cd reels-caption-app
npm install
```

## 3.1. Fetch bundled resources (one command, instead of hunting down each piece)

```bash
npm run fetch-resources
```

Downloads a single archive (`dev-resources.tar.gz`, ~1.1GB) from this repo's
[GitHub Releases](../../releases) and unpacks it straight into
`src-tauri/resources/` — everything below in one shot:

- `resources/bin/` — ffmpeg + ffprobe (section 2)
- `resources/llama/` — llama.cpp binaries + the Qwen2.5-0.5B-Instruct GGUF (section 2.3)
- `resources/tts-models/en/` — Piper's English voices (section 2.6)
- `resources/stt-models/ta/` — the converted Tamil Whisper checkpoint (section 2.2)
- `resources/voice-clone-models/converter/` — the OpenVoice V2 checkpoint (section 2.7)

**Why a release asset instead of Git — or Git LFS:** these are large, static
binaries that never change once fetched. Vendoring them as plain git blobs
would bloat every clone forever; Git LFS looks like the fix at first glance,
but it bills by *both* storage and bandwidth *per clone*, regardless of how
often the file actually changes — a single fresh clone of this repo would
already exceed GitHub's free 1GB/month LFS bandwidth quota on its own. A
release asset has none of that: a flat, generous 2GB-per-file limit, no
recurring bandwidth accounting for the repo, and it's just a plain HTTPS
download. That's exactly why the same mechanism is used to distribute the
built `.msi`/`.dmg`/`.AppImage` installers themselves (section 6.1).

This step needs a `v0.1.0`-or-later release to already exist with a
`dev-resources.tar.gz` asset attached (true for this repo from its first
release onward). If you'd rather fetch pieces individually — or you're
building for a platform this script doesn't cover — every section below
still documents the direct download/conversion steps for that one piece.

## 4. Set up Firebase authentication (required — this is the app's first screen)

The app now gates everything behind a login/signup screen (`src/components/auth/AuthScreen.jsx`),
backed by Firebase Auth's email/password provider — a deliberate exception to this project's
local-first design elsewhere (STT, LLM, all the media tools run with no account and no network).
Without a real Firebase project configured, the app can't get past this screen at all.

Only Firebase Auth's own native profile fields are used — `displayName` and `photoURL` — no
Firestore or other database. Signup asks for a display name, email, password, confirm password,
and an optional profile photo URL; nothing else is stored server-side.

1. Create a project at [console.firebase.google.com](https://console.firebase.google.com).
2. **Build → Authentication → Sign-in method** — enable the **Email/Password** provider.
3. **Project settings → your apps → add a Web app** — copy the resulting config object.
4. Copy `.env.example` to `.env` and fill in the values from that config:
   ```bash
   cp .env.example .env
   ```
   ```
   VITE_FIREBASE_API_KEY=...
   VITE_FIREBASE_AUTH_DOMAIN=...
   VITE_FIREBASE_PROJECT_ID=...
   VITE_FIREBASE_STORAGE_BUCKET=...
   VITE_FIREBASE_MESSAGING_SENDER_ID=...
   VITE_FIREBASE_APP_ID=...
   ```
   These values aren't secret — a Firebase web config is meant to be public; access control is
   enforced by Firebase's own rules, not by hiding this — but `.env` is still gitignored so each
   developer/deployment can point at their own project without editing source.

`src/context/AuthContext.jsx` wraps the whole app (`main.jsx`) and tracks the session via
Firebase's `onAuthStateChanged`; `App.jsx` renders `<AuthScreen />` while logged out, a brief
loading state while that initial check runs, and the existing `<AppShell />` once a session
exists. A logged-in user's name/photo (if set) and a **Log out** button appear in the menu bar
(`MenuBar.jsx`).

**Tauri-specific notes:**
- `tauri.conf.json`'s CSP is `null` (unrestricted), so there's nothing to configure for Firebase's
  network calls.
- Session persistence across app restarts relies on WebView2's local storage, which works out of
  the box.
- There's no offline fallback — if there's no internet on launch, the app can't get past the login
  screen. This is the real, ongoing cost of this feature versus everything else in this codebase.

## 5. Run in development (desktop)

```bash
npm run tauri dev
```

This opens a native window. The default flow is deliberately automatic —
most of it happens without pressing anything:

1. **Choose a video** — Media Pool sidebar (left), a native file picker
   (`@tauri-apps/plugin-dialog`).
2. **Transcription starts immediately** — no separate "Run pipeline"
   button, and no language picker either. `App.jsx`'s `pickVideo()` calls
   straight into the pipeline the instant a file is chosen: extracts
   16kHz mono audio with ffmpeg, **detects the spoken language**
   (`stt::detect_spoken_language` — a small `Systran/faster-whisper-tiny`
   checkpoint, used purely for its `detect_language()` call), then
   transcribes with whichever model matches — Parakeet for English, an
   Indic Whisper fine-tune for a supported Indian language (see section
   2). A processing banner (`shell/TranscribeStatus.jsx`) shows progress;
   the transcript (`shell/TranscriptPanel.jsx`, right column) fills in as
   soon as it's done, with the detected language shown alongside the word
   count. If the detected language isn't one this app has a model for,
   transcription fails with a clear error naming what was detected and
   what's actually supported — see section 7 for the full mechanism.

   **A video with no audio track is a normal case here, not an error** —
   real content, since B-roll meant to get a generated voice-over (step 3
   below) is exactly what this app's Voice-over feature is for.
   `ffmpeg::has_audio_stream` checks before extraction and skips straight
   to an empty transcript with a plain "no audio track found" message
   instead of reaching ffmpeg's own audio-extraction command, which
   previously failed with "Output file does not contain any stream" — a
   real reported bug, and a rough one specifically because transcription
   now auto-runs on upload: that raw ffmpeg stack trace was the very
   first thing a user saw after picking a silent video.
3. **Contextual tools, not a long sidebar list** — directly below the
   video/waveform (`shell/MainPanel.jsx`): a **Voice-over** section
   (upload a recording, or write a script to generate one locally —
   section 2.6) and a row of compact cards (`shell/ToolCard.jsx`) for
   Loudness, Music Ducking, and Title/Hashtags/Emoji (AI) — collapsed by
   default, expanding in place to the tool's existing panel rather than
   navigating to a separate screen. Everything else (caption style,
   silence removal, vocal emphasis, speaker diarization, exporting a
   muxed voice-over-sync file, plain keyword extraction) lives behind
   **More options** (`shell/MoreOptionsModal.jsx`) — reachable, not
   removed, just not competing for attention by default.
4. **Burn captions & save video** — the one button meant to be
   unmissable: a floating, always-visible **Burn & Export**
   (`shell/BurnExportButton.jsx`, fixed bottom-right regardless of
   scroll position). Prompts for a save location, generates an `.ass`
   subtitle file from the styled transcript, and re-encodes the video
   with ffmpeg's `ass` filter (libass) to burn the captions in.

The Developer menu (top bar) still has the original two checks — **Say
hello from Rust** (confirms the JS ↔ Rust bridge) and **Check ffmpeg
version** (confirms Rust can shell out to ffmpeg) — unchanged, just no
longer part of the main flow.

If ffmpeg or the `stt` conda env aren't found, you'll see a clear error
message instead of a crash — that's intentional so you can debug setup
issues early.

## 6. Add an app icon (needed before bundling, not needed for `dev`)

`src-tauri/icons/` already has a full generated icon set checked in. To
regenerate it from a different logo (one square PNG, 1024x1024
recommended):
```bash
npm run tauri icon path/to/your-logo.png
```
`tauri dev` works fine without an icon at all; `tauri build` needs one —
this repo already has one, so building "just works."

## 6.1. Build & distribute (Windows / macOS / Linux)

`npm run tauri build` runs `vite build` then compiles the Rust backend in
release mode and bundles installers for **whatever OS you run it on** —
Tauri does not cross-compile a GUI app for a different OS from one
machine (WebView2/WKWebView/webkit2gtk are all platform-native). To ship
installers for all three, build on all three, natively or via CI (see
"CI matrix" below) — there's no single command that produces all of them
from one machine.

`bundle.targets: "all"` in `tauri.conf.json` means "every installer format
this OS supports," not "every OS" — so `npm run tauri build` alone already
produces every relevant format for whichever platform runs it.

### Windows — MSI specifically

```bash
npm run tauri build -- --bundles msi
```
Output: `src-tauri/target/release/bundle/msi/Reels Caption App_0.1.0_x64_en-US.msi`.
Drop `-- --bundles msi` to also get the NSIS `.exe` installer
(`bundle/nsis/*-setup.exe`) alongside it — `targets: "all"` builds both by
default. First MSI build downloads the WiX Toolset v3 automatically
(needs internet, one-time, cached after); `.exe`/NSIS similarly downloads
`makensis`.

### macOS

Run the same command on a Mac:
```bash
npm run tauri build
```
Output: `src-tauri/target/release/bundle/dmg/*.dmg` and `macos/*.app`.
Needs Xcode Command Line Tools (`xcode-select --install`). An unsigned
`.app`/`.dmg` triggers Gatekeeper's "unidentified developer" warning on
first launch (right-click → Open bypasses it) — real code signing +
notarization needs an active Apple Developer account, out of scope for a
test build.

### Linux

Same command on a Linux machine:
```bash
npm run tauri build
```
Output depends on what's installed on the build machine: `.deb`
(`bundle/deb/*.deb`, needs `dpkg`), `.rpm` (`bundle/rpm/*.rpm`, needs
`rpmbuild`), and `.AppImage` (`bundle/appimage/*.AppImage`, portable,
runs without installing). Needs `webkit2gtk`/`libayatana-appindicator`
and friends — see
[Tauri's Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux)
for the exact package list per distro.

### CI matrix (the practical way to get all three from one push)

[`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action) is
the standard GitHub Actions workflow for this: a 3-OS build matrix
(`windows-latest`, `macos-latest`, `ubuntu-latest`), each running `npm run
tauri build` natively and uploading its own installers as release assets
— not set up in this repo yet, but the natural next step if testing
outgrows manual per-OS builds.

### What a fresh install actually gets you

The installer bundles ffmpeg/ffprobe, the local LLM (llama.cpp +
Qwen2.5-0.5B-Instruct), Piper's English voices, and the Tamil font —
**everything needed for caption styling/burning and title/hashtag
generation works immediately after install, no setup.** Speech-to-text,
voiceover generation, and voice cloning need their conda environments set
up separately by whoever's testing (sections 2, 2.4, 2.6, 2.7) — those
models are large and per-language/per-feature, so they're deliberately
not bundled into the installer (same reasoning as the Indic Whisper
checkpoints in `resources/stt-models/README.md`). Worth saying plainly to
anyone you hand this to: **installing the MSI alone does not give you
working transcription** — mention the conda setup steps, or they'll hit
"STT engine not found" the moment they upload a video.

### Before a real public release

`identifier` in `tauri.conf.json` is still the scaffold's placeholder
(`com.yourname.reelscaptionapp`) — fine for a one-off test build, but
worth setting to something real (reverse-DNS, e.g. `com.yourcompany.reels`)
before this identifier ends up baked into update metadata or app data
paths on testers' machines, since changing it later means a fresh install
path, not an in-place upgrade.

## 7. Model selection & language auto-detection (no dropdown anywhere)

Neither Parakeet nor an Indic Whisper checkpoint need a manual
fetch/quantize/bundle step the way whisper.cpp's GGML files once did —
Parakeet downloads and caches itself automatically via `huggingface-hub`
on first use, and each Indic model is converted once (section 2.2) and
then just sits in `resources/stt-models/`.

### 7.1. What's actually supported today: English + Tamil

This app supports **English and Tamil**, full stop — not "English, Tamil
and other Indic languages." The architecture (`stt::INDIC_LANGUAGES`,
`tts::TTS_INDIC_LANGUAGES`) is built to make adding another Indian
language straightforward, but as of this writing Tamil is the only one
actually wired up on either the transcription or the voiceover side.
`src::stt::get_supported_languages` is the single source of truth the UI
reads from (Sidebar's "Supported languages" line) — update
`INDIC_LANGUAGES` there (and `TTS_INDIC_LANGUAGES` in `tts.rs`) together
when a new language is added, rather than hardcoding the list anywhere
else.

### 7.2. No language picker: both directions auto-detect

There is no language dropdown or pill switch anywhere in this app — every
past version of this UI had one, and all of them are gone now:

- **Speech-to-text** (`pipeline::transcribe_with_auto_language`): after
  extracting audio, `stt::detect_spoken_language` identifies the spoken
  language via a small pre-converted `Systran/faster-whisper-tiny`
  checkpoint (`stt/detect_language.py`), used purely for its
  `detect_language()` call — never for the actual transcription, which
  stays on Parakeet/the Indic Whisper fine-tunes (too small/inaccurate
  for that job, but language ID is a much easier task and this size is
  trustworthy for it — verified directly on this project's own test
  audio: 98.7% confidence on English, 95% on Tamil). The detected code is
  then routed to whichever model matches, same as the old dropdown did
  manually. A detected language with no matching model is a clear error
  naming what was detected and what's supported (`stt::supported_language_names`),
  not a silent misroute or a confusing downstream crash. Runs in the same
  `stt` conda env as transcription (already depends on `faster-whisper`)
  — no new environment needed. Used by both `run_pipeline` (the main
  clip) and `transcribe_audio_file` (re-transcribing a voiceover).
- **Text-to-speech** (`src/lib/languages.js`'s `detectTextLanguage`):
  the voiceover script's language is inferred client-side from the text
  itself before calling `generate_voiceover` — a zero-dependency Unicode
  script-range heuristic (Tamil's dedicated block, U+0B80–U+0BFF; a
  majority of Tamil-range characters routes to MMS-TTS, otherwise
  Piper/English). Reliable specifically because English vs. Tamil is a
  script-level distinction here, not a same-script language call that
  would need real NLP — extend the range map (and `TTS_INDIC_LANGUAGES`
  in `tts.rs`) together if a non-Latin-script language is added that
  isn't distinguishable this way.

Adding another Indian language means converting an STT checkpoint for it
(section 2.2), adding it to `stt::INDIC_LANGUAGES` and
`tts::TTS_INDIC_LANGUAGES`, and — only if it doesn't already have its own
Unicode block distinguishable from existing languages — extending
`detectTextLanguage`'s script-range map.

There's also no separate "transcription quality" tier: unlike the old
whisper.cpp GGML files and WhisperX's swappable faster-whisper sizes,
each language here has exactly one model, already chosen and quantized
for good CPU speed — there's no smaller/larger variant to trade off.

Known limitation: RTL scripts (Arabic, Hebrew) aren't specifically
handled in the ASS caption output yet — the `ass` filter/libass path this
app uses hasn't been verified to render right-to-left text correctly.

### 7.2. Tanglish slang normalization (optional)

The Transcribe panel's **Normalize Tanglish slang spelling** checkbox
(off by default — preserves whatever spelling the STT engine produced)
runs `slang::normalize_words` over the transcript after transcription:
a small hand-built dictionary in `src-tauri/src/slang.rs` collapses
known spelling variants of the same colloquial word to one canonical
form (e.g. `sema`/`semmaya` → `semma`, `macha`/`machaa` → `machi`),
preserving punctuation, capitalization style, and word timing. Pure
local string matching — no model, no network call. Add more variant
groups to `VARIANT_GROUPS` in `slang.rs` as you find them; it's a flat
list, no retraining or reprocessing needed.

## 8. Build a distributable desktop app

```bash
npm run tauri build
```
Outputs installers in `src-tauri/target/release/bundle/` — `.dmg`/`.app`
on macOS, `.msi`/`.exe` on Windows, `.deb`/`.AppImage` on Linux.

### 8.1. Making it fully self-contained (no end-user install steps)

By default `tauri build` still expects `ffmpeg`/`ffprobe` on PATH and a
system conda `stt` env, same as `tauri dev`. To bundle ffmpeg, llama.cpp,
and the Qwen2.5-0.5B-Instruct model:

1. **ffmpeg/ffprobe**: drop `ffmpeg.exe`/`ffprobe.exe` into
   `src-tauri/resources/bin/` — see `resources/bin/README.md`.
   `src-tauri/src/bin_paths.rs` resolves these automatically (bundled
   resource first, PATH fallback second) at both `dev` and `build` time.
2. **llama.cpp + Qwen2.5-0.5B-Instruct**: already bundled by design — see section 2.3.
   `resources/llama/` ships in the installer like any other resource
   (`tauri.conf.json`'s `bundle.resources`), no separate conda env or
   account needed for this piece specifically.

None of this changes `tauri dev` — an empty `resources/bin/` just means
this step falls back to PATH, same as before.

**The `stt` conda env is not bundled** (unlike the old MFA "aligner" env,
which could be `conda-pack`ed into the installer). Its dependency surface
is much lighter than it used to be — dropping WhisperX (see section 2.2)
means it's just `faster-whisper`, `onnx-asr`, and `soundfile` now, no
PyTorch/transformers/torchaudio — but end users still need a system conda
`stt` env set up per section 2 above. This is a real regression in "zero
end-user setup" versus what the old MFA-bundling path offered; worth
revisiting `conda-pack` now that the dependency list is small enough to
make that more practical than it was before.

---

## 9. Adding iOS and Android

Tauri 2.0 supports mobile natively, from the same Rust + React codebase.

### One-time mobile setup
```bash
# Android: install Android Studio + SDK/NDK first, then:
npm run tauri android init

# iOS: install Xcode first (macOS only — Apple requires building on Mac), then:
npm run tauri ios init
```

### Run on a simulator/emulator
```bash
npm run tauri android dev
npm run tauri ios dev
```

### Build a release
```bash
npm run tauri android build
npm run tauri ios build
```

### ⚠️ The mobile catch: ffmpeg, the STT engines, and the LLM service

`check_ffmpeg` and the burn pipeline (`captions.rs`) shell out to
`ffmpeg` as a **separate CLI process**; `pipeline.rs`'s transcription
step shells out to a **conda-managed Python process** (Parakeet or
Indic Whisper, see `stt.rs`); `content_ideas.rs` talks to a **local HTTP server**
(`llama-server.exe`, see `llm.rs`). iOS and Android don't allow apps to
spawn subprocesses or ship arbitrary CLI binaries — everything has to run
**in-process** via compiled libraries.

- **ffmpeg → mobile:** use [`ffmpeg-kit`](https://github.com/arthenica/ffmpeg-kit) (has iOS/Android builds you link against), called via Rust FFI. Unaffected by this project's transcription rewrites.
- **Parakeet/Indic Whisper → mobile:** no equivalent path exists today for
  either side, but for different reasons now that the Indic path no
  longer needs PyTorch/transformers/torchaudio (see section 2.2) — its
  remaining dependency is CTranslate2 (a C++ inference engine, not a
  Python-only one), whose mobile build/FFI story hasn't been investigated
  yet. Parakeet is closer: it already runs on plain ONNX Runtime, and
  ONNX Runtime does ship confirmed mobile builds — an
  ONNX-Runtime-Mobile-based English-only path is plausible future work,
  just not implemented yet.
- **llama.cpp → mobile:** actually the best-positioned piece for this,
  ironically — llama.cpp has real iOS/Android builds and is designed for
  on-device inference, including phones. Not wired up yet, but the
  llama.cpp project itself removes most of the FFI-binding work the other
  two pieces would need from scratch.

This is a real regression versus the old whisper.cpp-based pipeline's
mobile story (which had a credible, if undone, FFI path via
`whisper-rs`) for the STT half specifically — accepted deliberately in
exchange for Parakeet/Indic Whisper's desktop-side accuracy gains over a
generic multilingual model, since this project targets desktop first
(see the top-level README intro).

---

## 10. Suggested next build steps

Done in this scaffold:
- ~~Add a file picker so users can select a video.~~ — `pickVideo()` in `src/App.jsx` + `@tauri-apps/plugin-dialog`.
- ~~Add a Rust command that runs the full pipeline: extract audio → transcribe → return word-level timestamps as JSON.~~ — `run_pipeline` in `src-tauri/src/pipeline.rs`.
- ~~Build the caption-styling UI in React.~~ — `src/components/CaptionStyleEditor.jsx`.
- ~~Add a Rust command that burns styled captions back onto the video.~~ — `burn_captions` in `src-tauri/src/captions.rs`, via ffmpeg's `ass` filter.
- ~~Let users edit transcript text/timestamps in the UI before burning.~~ — `src/components/TranscriptEditor.jsx`.
- ~~Replace whisper.cpp + MFA with a single, more reliable transcription+alignment tool.~~ — WhisperX, then Parakeet/IndicWhisper (section 2 above).
- ~~Add a local LLM-powered feature.~~ — title/description/hashtag/emoji generation via llama.cpp + Qwen2.5-0.5B-Instruct (`llm.rs`/`content_ideas.rs`), section 2.3.
- ~~Add sign-up/login gating the app.~~ — Firebase Auth (email/password, native `displayName`/`photoURL` fields only), `AuthContext.jsx`/`AuthScreen.jsx`, section 4.
- ~~Add a text-to-speech voiceover feature.~~ — Piper (English) + MMS-TTS (Indian languages), `tts.rs`/`shell/VoiceoverSection.jsx`, section 2.6.
- ~~Play a generated voiceover synced to mouth movement directly in the video preview, and match its delivery to the script's tone.~~ — `compute_voiceover_offset`/`vosync.rs` + `VideoPreview.jsx`'s dual-media sync, and emotion-preset rate/pitch via `tts.rs`, section 2.6.
- ~~Redesign the UI around "mostly automatic, fewer visible options by default."~~ — transcription auto-starts on upload, a long sidebar Tools list became contextual cards below the video plus a "More options" modal, and Burn & Export became a permanent floating button — `shell/MainPanel.jsx`, `shell/ToolCard.jsx`, `shell/TranscribeStatus.jsx`, `shell/MoreOptionsModal.jsx`, `shell/BurnExportButton.jsx`, section 5.
- ~~Fix Burn & Export ignoring an active voiceover.~~ — `burn_captions` gained `voiceover_path`/`voiceover_offset_seconds` params (swaps the audio track instead of copying the original), and the voiceover gets re-transcribed (`pipeline::transcribe_audio_file`) so its own words become the working transcript — `captions.rs`, `pipeline.rs`, `App.jsx`, section 2.6.
- ~~Fix a raw ffmpeg crash when auto-transcription hits a silent video.~~ — `ffmpeg::has_audio_stream` checks before extraction; a video with no audio track (real content — B-roll meant for a generated voice-over) now gets a plain "no audio track found" message and an empty transcript instead of ffmpeg's own "Output file does not contain any stream" error, section 5.
- ~~Remove every language dropdown/pill and auto-detect language instead, for both transcription and voiceover generation.~~ — `stt::detect_spoken_language` (audio-based, via a small `Systran/faster-whisper-tiny` checkpoint) for speech-to-text, `src/lib/languages.js`'s `detectTextLanguage` (Unicode script-range heuristic) for text-to-speech, `pipeline.rs`/`stt.rs`/`shell/TranscribeStatus.jsx`/`shell/VoiceoverSection.jsx`, section 7.
- ~~Make a generated voiceover actually sound like the original speaker, not just a fixed default voice.~~ — OpenVoice V2's `ToneColorConverter` (new `voice_clone.rs`, its own conda env) reshapes the synthesized clip's timbre to match a reference clip pulled from the loaded video, falling back to a gender-matched Piper voice or the plain default when cloning isn't possible — `tts.rs`'s `resolve_voice_reference`/`generate_voiceover`, section 2.7.
- ~~Fix the live preview playing the video's original audio underneath an active voiceover.~~ — `VideoPreview.jsx`'s `<video muted={!!voiceoverPath}>` only took effect at the element's initial mount, a documented React special-case for media elements (the `muted` JSX prop isn't re-applied to the DOM node on later re-renders) — since a voiceover is normally generated well after the video element already exists, the mute never actually landed. Fixed with an effect that sets `videoRef.current.muted` imperatively whenever `voiceoverPath` changes. The burned/exported file was never affected — that's ffmpeg re-encoding the audio track directly, not this preview element.
- ~~Make a fresh clone actually buildable without hunting down every model/binary by hand.~~ — `npm run fetch-resources` (`scripts/fetch-dev-resources.mjs`) downloads one ~1.1GB archive from this repo's GitHub Release and unpacks it into `src-tauri/resources/`, section 3.1. Also cleaned up real repo hygiene issues found along the way: OpenVoice's `se_extractor` was caching scratch audio/embeddings into a relative `processed/` directory that landed inside the working tree and got committed (`clone_voice.py` now pins it to a temp directory instead — see `voice_clone.rs`'s section 2.7); Git LFS was tried first for the large model files but dropped in favor of the release-asset approach once its per-clone bandwidth billing turned out to cost more than plain storage, given these files never change.

Still open:
1. Sidecar-bundle `ffmpeg` itself (https://v2.tauri.app/develop/sidecar/) so users don't need it on PATH.
2. Real-time WYSIWYG caption preview over the actual video frame, not just the style swatch.
3. A path to bundling the `stt` conda env for zero end-user setup — see section 8.1's regression note (llama.cpp/Qwen2.5-0.5B-Instruct is already bundled; the STT engines' env is the piece still missing this).
4. More Indian languages in `stt::INDIC_LANGUAGES` beyond Tamil — see `resources/stt-models/README.md`.

Everything above runs 100% locally — `llama-server.exe` is a *local* HTTP
server bound to `127.0.0.1` only, not a remote one, so this is still
no cloud calls and no per-user cloud cost.
