# KraftReel.App

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

## Quick start: setting up a new dev workstation

Everything below is documented in full in its own section — this is the
ordered checklist so a fresh machine doesn't need to read the whole README
front-to-back to know what to run. Each step links to the section with the
real detail (troubleshooting, why it's built this way, what's optional).

1. **Install prerequisites** — Node.js 18+, Rust, and your OS's Tauri
   build tools (section 1).
2. **Clone the repo, install JS dependencies:**
   ```bash
   git clone https://github.com/antorobin/reels-caption-app.git
   cd reels-caption-app
   npm install
   ```
   (section 3)
3. **Fetch the bundled binaries/models in one shot:**
   ```bash
   npm run fetch-resources
   ```
   Pulls ffmpeg, llama.cpp + the local LLM, Piper's English voice, the
   Tamil Whisper checkpoint, and the OpenVoice checkpoint from this repo's
   GitHub Release — no separate downloads to go hunt down (section 3.1).
4. **Get the Python environments** for whichever features you need — these
   install actual Python packages, not just files, so `fetch-resources`
   can't do this part. Either build the bundled relocatable envs (no
   conda, and this is also what a `tauri build` bundles — see section 8.1):
   ```bash
   node scripts/build-python-runtime.mjs --pack=core     # stt -- required for any transcription
   node scripts/build-python-runtime.mjs --pack=extras   # tts + media-ai + voice-clone
   ```
   …or set up system conda envs the old way (`stt` section 2, `tts`
   section 2.6, `media-ai` section 2.4, `voice-clone` section 2.7).
   `src-tauri/src/python_env.rs` prefers a bundled env and falls back to
   conda, so either works for `tauri dev`.
5. **Configure Firebase auth** — required; this is the app's first screen,
   nothing else works until this is set up (section 4).
6. **Run it:**
   ```bash
   npm run tauri dev
   ```
   (section 5)

That's a working dev setup. Building a distributable installer (`.msi`/
`.dmg`/`.AppImage`) is a separate, later step — section 6.1.

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

**Grew into "content strategy" (multiple options, and a free-text hint
input)** — requested directly: `generate_content_ideas` (one fixed
title/description/hashtags result, transcript-only, no way to steer it)
became `suggest_content_strategy` (3 distinct angle options — Educational/
how-to, Entertainment/relatable, Aspirational/emotional, matching
`music_gen.rs`'s own "name fixed categories explicitly" fix for the same
small-model near-duplicate problem — plus an optional free-text `hints`
field the creator can use to steer every option's topic, and a `language`
parameter finally threaded through the same way `suggest_background_music`
already had it). `hints` alone with an empty transcript is a real,
supported input, not just a nice-to-have: it's what a video generated
from scratch with no upload (section 10) seeds its content strategy from,
since there's no transcript yet at that point.

Verified directly against 4 real cases (English transcript alone,
transcript+hints, hints alone, Tamil transcript alone) before shipping —
two real, fixed issues and one real, unfixed limitation came out of that:
an instruction locking the 3 options into a fixed output order was tried
and dropped after real generations kept coming back in random order
anyway regardless of the instruction (each option's own `angle` field is
what actually identifies it now, nothing depends on array order); combining
a transcript with hints occasionally produced duplicate hooks across
options or literal placeholder-looking text in the `emoji` field instead
of a real pictograph (a new `sanitize_emoji` in `content_ideas.rs`, next
to the existing `sanitize_hashtag`, is the defensive backstop for the
latter). The one real *unfixed* finding — a Tamil transcript with no hints
can fabricate an entirely unrelated topic, confirmed across 3 separate
test runs each inventing a different fictional topic — is documented
honestly in the "Still open" list at the end of this file rather than
silently shipped as if it always works; giving hints alongside Tamil
content reliably anchors the topic even when transcript comprehension
itself fails, so that's the practical mitigation for now.

The per-language token budgeting this needs (the same 4096-token
`llama-server` context ceiling and Tamil-vs-English tokenizer cost gap
`suggest_background_music` already had to handle) was factored out of
`music_gen.rs` into a new shared `llm_budget.rs`, rather than growing a
second independent copy of those constants — see section 9.3's "Still
open" list for why that duplication used to exist.

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

## 2.6.1. AI-generated background music, matched to the speech (Music ducking)

"Music ducking" (`ducking.rs`/`DuckingPanel.jsx`, mixes a music bed under
the video's own audio, automatically lowering it during speech using the
transcript timing already on hand) used to only accept a music file the
user already had. It can now generate one instead — a "✨ Suggest music"
button sits next to the existing "Choose music file…" button in the same
panel; both just set the same `musicPath` the duck-level slider and mixing
step below already use, so nothing about the actual ducking changed.

**Model: Meta's MusicGen-Small (`facebook/musicgen-small`)**, an official
Hugging Face `transformers` model, text-conditioned, 300M params,
CPU-capable. TinyMusician (a smaller/faster distillation of MusicGen) was
looked at first — ruled out because it has no public checkpoint, pip
package, or GitHub implementation as of this writing, only a September 2025
arXiv paper (confirmed via web search, not assumed).

**No new conda environment.** The existing `tts` env (section 2.6 above)
already has `torch`+`transformers`+`scipy` for MMS-TTS — exactly what
MusicGen needs too. Verified directly before writing any wiring code: this
project's own `tts` env already had `transformers` 5.15.1, well past the
4.31 minimum MusicGen needs, so not even a version bump was required.
`facebook/musicgen-small` (~2.3GB) downloads on first use via
`transformers`' own Hugging Face Hub cache, the same lazy-download pattern
already relied on elsewhere in this app.

**Suggest, then pick — not generate-and-apply.** The original version
generated one bed from one LLM-written prompt and applied it immediately.
Changed after direct feedback: "✨ Suggest music" now generates 3 different
short (6s) preview clips via `suggest_background_music`, each playable
inline before committing to anything; picking one calls
`finalize_background_music`, which re-generates *only that one* prompt at
the real target length (looped if the video's longer than
`MAX_DIRECT_GENERATION_SECONDS`) and is what actually becomes the active
bed. Auditioning cheap short previews before paying full generation cost
for just the one that's actually wanted matters a lot here specifically
because of how slow this model is on CPU (below) — generating 3 full-length
candidates up front to throw 2 away would triple an already-expensive wait
for nothing.

**How the prompts are derived, and five iterations getting there:**
`music_gen.rs`'s `derive_music_prompt_suggestions` reuses `llm.rs`'s local
Qwen2.5-0.5B-Instruct service exactly like `content_ideas.rs` does for
titles/hashtags — but getting 3 good, genuinely different, purely
instrumental, culturally-aware suggestions out of a 0.5B model took five
iterations (the first three fixing real bugs, the last two adding
requested capability), each verified against the real model over HTTP
before shipping, not assumed to work from the prompt text alone:
1. A plain zero-shot instruction asking for an N-item JSON array made the
   model echo fragments of its own instructions back as the "suggestions"
   (`"purely instrumental"`, `"no vocals"`, `"no lyrics"`) — the same
   small-model limitation already documented for `tts.rs`'s emotion
   classifier, worse here since an array of genuinely distinct items is a
   harder structural ask than single-label classification.
2. One few-shot worked example (the fix that worked for the emotion
   classifier) produced real, distinct-sounding prompts — but a second test
   transcript (a memorial tribute) came back with a suggestion describing
   "a soft, emotive vocal, like a lead singer," directly violating "no
   vocals": a real functional bug, since that prompt handed to MusicGen
   would push it toward generating something vocal-like instead of a
   purely instrumental bed.
3. Strengthening the no-vocals wording alone fixed the vocal leakage but
   collapsed all 3 suggestions into near-duplicates of each other.
   Explicitly naming three instrumentation *categories* to fill (one
   acoustic/organic, one electronic/synth, one percussion/rhythm — see
   `MUSIC_PROMPT_SYSTEM`) fixed both at once: verified across three
   different-mood test transcripts (a DIY project, a memorial, a race
   countdown) with zero vocal-word leakage and three structurally distinct
   genres every time.
4. Requested directly: factor in the spoken language's own film/popular
   music culture too (Tamil cinema, English-language albums, etc), not just
   content/mood. Added a `Language:` field alongside the transcript and a
   third few-shot example (Tamil, leaning Carnatic/Kollywood-style
   instrumentation) — verified against a real Tamil transcript from this
   project's own earlier testing, and it correctly leaned South
   Indian/Carnatic-style instrumentation from the language label alone.
   Also found, by isolating it directly rather than guessing: a heavily
   **code-switched** (Tamil+English mid-sentence) transcript can derail
   this 0.5B model badly regardless of the language label — one real test
   about cooking made it describe the recipe itself ("warm biryani,
   comforting") instead of music entirely, while a plain-English
   translation of the identical content only degraded mildly. This is a
   real limitation of this specific lightweight local model's non-English/
   code-switched comprehension, not something fixed here (would need a
   translation step this app doesn't have) — documented honestly rather
   than silently shipped as if it always works.
5. Requested directly, again: go further than film/classical and name
   *regional folk* styles too — e.g. Gana, rhythmic percussion-driven
   street/folk music from North Chennai associated with working-class and
   mass-appeal themes. Named it explicitly in the instruction and swapped
   the Tamil example's percussion suggestion to demonstrate it. Verified
   against two real Tamil transcripts, not just one: an energetic
   working-class-story transcript correctly picked up Gana, while a calmer
   devotional one still correctly favored Carnatic/Kollywood instead — it's
   reading the content, not just parroting Gana regardless. One honest
   caveat found in the same test: on a harder transcript, the model
   occasionally mangled "Gana" into a garbled non-word ("Ghaannar-style")
   — rough in the UI's displayed suggestion text, but the surrounding
   descriptive words (percussion, rhythm, street music) still carry real
   meaning for MusicGen's own generation, so it doesn't break the actual
   audio the way the vocal-leakage bug did.

Mood-matching for any one suggestion still isn't perfect at this model
size — acceptable given the entire point of offering 3 auditioned options
is letting a human pick the one that actually fits, not trusting one AI
guess to nail it.

**The full transcript goes into deriving the suggestions — but only the
short result goes into MusicGen.** The LLM step sees the entire spoken
transcript plus the detected language; the actual `generate_music.py` call
never sees the transcript at all, only the short style sentence the LLM
wrote (MusicGen's text encoder is built for short style captions, not
conversational dialogue — feeding it a raw transcript would confuse it, not
help it). One real gap this surfaced, asked about directly and then fixed
rather than assumed away: `llm.rs` runs its local model with a 4096-token
context, and that's a hard limit — confirmed directly by sending an
oversized real request and getting back a plain HTTP 400
(`"exceeds the available context size"`), not silent truncation.
`content_ideas.rs` shares this same unbounded-transcript shape and would
hit the identical wall on a long enough video. Measuring the real
tokenizer (via `llama-server`'s own `/tokenize` endpoint) surfaced
something worth knowing on its own: **Tamil script costs roughly 8.6
tokens per word in this tokenizer, versus ~1.0 for English** — built
around Latin/CJK text, it falls back to a much less efficient encoding for
Tamil. That means a *moderate* Tamil video, not just an extreme-length one,
can hit the context limit. Fixed with `transcript_text_for_prompt`: a
transcript within a language-aware word budget goes through whole; a
longer one is sampled from its beginning, middle, and end (40/20/40) rather
than simply cut off at the end, so a style decision still reflects the
whole video rather than just its intro. Verified end-to-end against a real
1000-word simulated Tamil transcript (previously ~8,600 tokens on its own,
comfortably over the limit): truncated to ~310 words, the real measured
request came to 3,261 tokens, and generation succeeded.

**Real CPU generation speed, measured directly, not assumed:** MusicGen-
Small runs roughly **12x slower than real-time** on this project's own dev
machine — a 20-second clip took ~4m12s wall-clock with the model already
warm (no download in that run). That directly shaped
`MAX_DIRECT_GENERATION_SECONDS` (15s, capping the worst case around 3
minutes) — a longer video gets a 15s bed generated once and looped to fill
the remaining length (`ffmpeg::loop_audio_to_duration`, a new
`-stream_loop -1 -i bed -t <duration> -c copy` helper) rather than
generating the full length directly. Known v1 trade-off: the loop point
isn't crossfaded, so a hard seam can be audible on a short/percussive bed —
not addressed yet. The UI's busy label says plainly that this is slow
rather than showing a misleadingly generic spinner.

**Audible live in the preview, not just after a real export.** A picked or
finalized bed now plays directly alongside the loaded video in
`VideoPreview.jsx`, ducked in real time — the same live-preview pattern
section 2.6 already uses for a generated voiceover (a hidden `<audio>`
element kept in lockstep with the `<video>` element's play/pause/seek
events), extended here with a JS reimplementation of `ducking.rs`'s own
duck-volume curve (`speechWindowsFromWords`/`duckVolumeAt` in
`VideoPreview.jsx`, mirroring `speech_windows_from_words`/
`build_ducking_volume_expr` constant-for-constant) evaluated fresh every
frame instead of baked into an ffmpeg filter string. `musicPath`/`duckLevel`
are lifted to `App.jsx` for this the same way `voiceoverPath`/
`voiceoverOffset` already are. Verified the curve math directly (fed real
sample timestamps through both the ramp-up and ramp-down edges before
trusting it) rather than just eyeballing the numbers. Before this, "Music
ducking" only ever wrote a separate merged output file — a real reported
gap: after generating a bed, nothing made it audible until running "Add
music with ducking" and opening the result.

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

## 2.8. Recording a voice-over by speaking (mic input + global-hotkey dictation)

Two more ways to get a voice-over into a loaded video, alongside upload and
generate-from-script (section 2.6):

- **In-app**: VoiceoverSection's "Record from mic" mode — a push-to-talk
  button using the standard `getUserMedia`/`MediaRecorder` Web APIs (no new
  native audio dependency), converted to the same 16kHz mono WAV every
  other audio path here expects via `mic_recording.rs` (shells out to the
  already-bundled ffmpeg). Its own checkbox decides whether the recording
  **replaces the video's audio** (goes through the same sync-to-mouth-
  movement + re-transcribe flow as upload/generate) or **only updates the
  captions**, leaving the video's own audio untouched — off by default, so
  a recording never silently swaps the audio unless asked.
- **Global hotkey, from anywhere in the OS**: `Ctrl+Shift+D` pops up a
  small floating HUD (`dictation.rs` + `DictationHud.jsx`) — even with the
  main window closed/tray-only. Speak, then press the hotkey again (or
  Enter inside the HUD) to finish, or Escape to cancel. Text grows live in
  the HUD itself while you talk — it uses the exact same live-dictation
  backend as "Live dictation" below, not a separate record-then-batch
  pass. This path always updates captions only (no voiceover-replace
  option — that's a more deliberate action better suited to the in-app
  checkbox than a fast global shortcut). If no video is loaded when it
  finishes, the main window says so rather than silently discarding the
  recording — the HUD itself has no way to know that; it's a separate
  window.

### Live dictation: text appearing as you speak (separate workflow, both by button and by global hotkey)

A third, genuinely different path: **`LiveDictationPanel.jsx`** ("Live
dictation (beta)", shown right below Voice-over) shows captions growing
while you talk, instead of waiting for a full recording to finish. The
`Ctrl+Shift+D` global hotkey documented above in this same section uses
this exact same backend too, so both entry points behave identically.
Deliberately kept separate
from everything above — it never touches the video's audio, only the
captions, via the same `onCaptionsFromRecording` callback.

**How it actually works — and why, after two real iterations**:

1. *First attempt*: a true incremental streaming recognizer
   ([`sherpa-onnx`](https://github.com/k2-fsa/sherpa-onnx) + native mic
   capture via [`cpal`](https://github.com/RustAudio/cpal)) — genuinely
   smooth per-word live text, but English-only. Also hit a real build
   issue along the way worth remembering: `sherpa-onnx`'s prebuilt Windows
   *static* library is MSVC-built and can't link against this project's
   GNU/MinGW toolchain at all (a `cargo build`/`tauri dev` failure `cargo
   check` alone never catches, since it doesn't perform the actual link
   step) — fixed at the time by switching to `sherpa-onnx`'s `shared`
   (DLL) feature instead, since dynamic linking against a plain C API
   turned out to be far more toolchain-portable than static-linking C++
   object code.
2. *Why it was replaced anyway*: no streaming-capable ASR model exists for
   Tamil (or Tamil+English code-switching) today — confirmed via
   [k2-fsa/sherpa-onnx's own project discussion](https://github.com/k2-fsa/sherpa-onnx/discussions/3199)
   asking exactly this question. Since this app's whole point is
   Tamil+English support, "smooth but English-only" wasn't the right
   trade-off, so `sherpa-onnx` was dropped entirely.
3. **What's actually running now**: `streaming_stt.rs` still captures
   audio natively via `cpal` (that part of the brief — "use the OS's own
   capability" — didn't need to change), and recognition reuses the
   *exact same* segment → classify → route-to-Parakeet-or-Tamil-Whisper →
   stitch logic `mixed_language.rs` already proves out for whole videos
   (section 7.4) — `subdivide_long_segments`/`resolve_segment_languages`/
   `merge_into_chunks` are shared directly (`pub(crate)`), not
   reimplemented. Code-switching works here for free, because it's the
   same code, not a second implementation.
4. **A second real performance bug, found the same way (try it, watch it
   fail, fix the actual cause)**: the first version of this called
   `mixed_language::transcribe_with_language_detection` directly every
   ~2.5s — which spawns a fresh Python process and reloads the whole ASR
   model from disk *on every call*. That easily took far longer than the
   2.5s interval itself, which is exactly why it didn't feel live at all.
   Fixed with `stt/live_worker.py` (`LiveWorkerState` in `streaming_stt.rs`)
   — one persistent Python process that loads the language-ID model,
   Parakeet, and the Tamil checkpoint *once*, stays resident for the whole
   app session (not just one dictation session — started lazily on first
   use and reused after that), and serves repeated requests over a plain
   stdin/stdout JSON-line protocol with no reload cost. Only the two
   actual model-inference calls route through the worker; the Rust-side
   segmentation/merge orchestration above is completely unchanged. The
   main batch pipeline (`pipeline.rs`) still uses the one-shot scripts
   directly and is untouched by any of this — it only pays the load cost
   once per video anyway. Text still grows every ~2.5s, not instantly per
   word (a full re-transcription of the growing buffer each cycle, not
   incremental decoding) — that residual gap is inherent to the chunked
   approach, not the model-reload bug this fixed.
5. No separate model to download or bundle — this reuses whatever the
   main transcription pipeline already has set up (the `stt` conda env,
   plus the Tamil checkpoint if you want Tamil captions to work here too).

**Also fixed while chasing the above**: two real bugs found through actual
live testing, not hypothetical. A backend session left in memory could
permanently block starting a new one (e.g. after a frontend-only reload
that didn't restart the Rust process) — `start_live_dictation` now checks
whether the stale session's capture thread has actually finished and
self-heals instead of erroring. And `stop_live_dictation` used to *wait*
for the final transcription pass before returning, which could leave the
whole HUD frozen on "Finishing…" with nothing clickable if that pass was
slow (or, rarely, stuck) — it now signals stop and returns immediately,
with the real result arriving later via a `"live-dictation-final"` event;
every phase of the HUD also now has an always-visible close button, so it
can never get stuck again regardless of what the backend is doing.

**Honesty check**: the streaming-recognizer build issue, the worker
replacement, and both bugs above were all found/fixed via real `cargo
build` runs and real testing, not assumed away — but real transcription
quality and real code-switching behavior in this specific chunked setup
still haven't been exercised end to end in the environment this was built
in, for lack of a real mic there. Try it and report back what needs
fixing.

## 2.9. Semantic project search (cosine similarity / vector embeddings)

The sidebar's project list search box ranks projects by actual meaning
(title + description + hashtags + a transcript snippet), not just a
substring match on the filename. Two real pieces, both verified directly
against this project's exact setup before writing any application code:

- **`sqlite-vec`** (a real SQLite virtual-table extension, `vec0`) does the
  cosine-distance KNN ranking, embedded directly into the same `library.db`
  the rest of the project library already uses — no separate vector
  database. Confirmed with a standalone probe against this project's exact
  `rusqlite` version (0.32): a real `vec0` table with
  `distance_metric=cosine`, three test embeddings, and a KNN query that
  correctly ranked two similar embeddings above a dissimilar one — real
  ranking, not insertion order.
- **`all-MiniLM-L6-v2`** (via `sentence-transformers`, in the existing
  `tts` conda env — no new environment) turns text into a 384-dimension
  embedding. Confirmed directly: two Kanyakumari-related sentences scored
  0.62 cosine similarity against each other, versus 0.13 against an
  unrelated chocolate-cake sentence — real semantic ranking, not a
  coincidence of shared words.

**Setup** (one more package in the already-existing `tts` env):

```bash
conda run -n tts pip install sentence-transformers
```

**Why a persistent local server, not a one-shot script per call**: every
other Python integration in this app (`tts.rs`, `music_gen.rs`) spawns a
fresh process per call, because those operations already take seconds to
minutes — a fresh process's startup cost is noise. Embedding a query while
someone is actively typing in a search box is different: loading
`all-MiniLM-L6-v2` takes ~19s (measured directly), which is fine to pay
*once* but not on every keystroke. `embeddings.rs` + `tts/embed_server.py`
instead follow `llm.rs`'s llama-server pattern exactly — a small HTTP
server (stdlib `http.server`, no new web-framework dependency) started
once, lazily, on first use, kept running for the app's lifetime, on its own
port (8735, separate from llama-server's 8734).

Re-embedding a project (so search reflects its current title/description)
happens automatically at a couple of natural checkpoints — after a fresh
content-strategy generation, and ~8s after the last manual title/
description/hashtag edit — not on every keystroke or every 1s autosave
tick, the same "don't hammer the model server for no reason" reasoning as
the debounce above.

## 2.10. Caption theme library (bundled professional presets)

`src/lib/themes.js` ships 37 named caption-style presets, grouped into 13
categories (Bold & Punchy, Karaoke & Highlight, Cascade Spotlight, Minimal
& Clean, Cinematic & Editorial, Energetic & Neon, Frosted & Soft, Boxed &
Framed, Retro & Synthwave, Vibrant Colorful, Comic & Pop Art, Elegant &
Script, Corporate & Lower-Third), selectable via a filterable card grid at
the top of `CaptionStyleEditor.jsx` — picking one replaces the whole
style; the granular controls below stay available to fine-tune
afterward. Designed by researching the caption-style conventions used
across popular short-form-video/captioning tools and commercial subtitle-
template marketplaces, plus two open-source reference projects
([remotion-captions-themes](https://github.com/vshukla7/remotion-captions-themes),
[pycaps](https://github.com/francozanardi/pycaps)) and a general survey of
Adobe Stock's and Envato Elements' subtitle-template catalogs, for
named-style/archetype inspiration — no code, design, or asset from any of
these was copied, only the general visual archetype (e.g. "synthwave
neon," "comic-book burst," "elegant serif") each surfaced.

Every theme is built entirely from `CaptionStyle` fields the ASS/libass
burn pipeline (section intro above, `captions.rs`/`segments.rs`) already
supports — no theme here relies on emoji, true gradient fills, real
glow/blur, non-center alignment, or rounded box corners, none of which
this renderer can currently do (see "Not attempted" below).

**A real, verified bug was found and fixed as part of building this
library**: `animation: "karaoke"` was silently invisible — `build_style_line`
emitted the same ASS color for PrimaryColour and SecondaryColour, so
`\k` tags carried only inert timing, no visible fill sweep (confirmed via
a real ffmpeg+libass render: the burned frame was byte-identical before
and after a word's `\k` duration elapsed). Fixed by giving SecondaryColour
a fixed muted gray (`KARAOKE_SECONDARY_COLOUR`, matching the pre-word gray
`"highlight"`'s animation already used) distinct from PrimaryColour — one
constant, no `CaptionStyle` schema change. Every karaoke-based theme
(Clean Classic, Golden Karaoke, Monoline Flow) gets a real, visible
word-by-word reveal as a result.

**4 new bundled display fonts**, added to `FONT_OPTIONS` alongside the
existing system fonts, sourced from Google Fonts' OFL-licensed
`google/fonts` repo (license files under `resources/fonts-license/`,
matching Noto Sans Tamil's existing precedent in 2.5 above):
- **Montserrat** (Bold) — bundled as a static instance, not the upstream
  variable font: the variable file's own default named instance is
  "Montserrat Thin", not "Montserrat" (confirmed via `fontTools`), which
  would have made font-family matching by name unreliable. Pinned to
  weight 700 via `fonttools varLib.instancer` (`updateFontNames=True`,
  which correctly rewrites the name table to "Montserrat"/"Bold" for the
  pinned instance) rather than shipping the ambiguous variable font.
- **Bebas Neue**, **Anton** — single static weight each (both fonts only
  ship one weight upstream).
- **Poppins** — both Regular and Bold static weights bundled (themes use
  both).
- **Playfair Display** (Regular + Italic, variable-weight 400-900) — added
  for the Elegant & Script category; unlike Montserrat above, its variable
  font's own default instance already correctly declares "Playfair
  Display"/"Regular" (confirmed via `fontTools`), so the raw upstream
  variable files are bundled directly, no instancer-pinning needed.

Same dual-path integration as Noto Sans Tamil: the `.ttf` files live in
`src-tauri/resources/fonts/` for the ffmpeg/libass burn path (via
`fonts_dir()`/`fontsdir`), and are mirrored into `src/assets/fonts/` with
their own `@font-face` blocks in `styles.css` for the in-app WebView
preview (the theme picker's live swatches, the style editor's own
preview) — a separate rendering path from the burn output. Unlike Noto
Sans Tamil, these are deliberately-selected display faces for specific
themes, not general Unicode-fallback fonts, so each gets its own
`@font-face` only and is not added to `:root`'s base font stack.

**Verified for real** (not just assumed from the theme JSON): burned real
test clips through the actual bundled `fontsdir`, for one theme per new
font and one per new mechanic (Golden Karaoke's gold reveal after the
fix; Grape Box's Poppins-on-purple-box cascade look; Beast Mode's Anton
+ heavy shadow; Bebas Trending's wide letter-spacing) — inspected the
real rendered frames, not the generated ASS text.

**Not attempted** (flagged, not silently skipped): emoji-in-caption
rendering (a real gap in `captions.rs` — whether libass can even render
color emoji through the `ass` filter with a bundled color-emoji font is
unverified); true gradient text fill, true glow/blur, and true glitch/
scanline/chromatic-fringing textures (not an ASS/libass capability in this
pipeline — "Gaming Neon"/"Frosted Glass"/"Synthwave Neon"/"Retro Arcade"
approximate those vibes with saturated color pairings, heavy shadow, and a
low-opacity box instead, honestly, rather than claiming a literal VHS/neon
effect); left/right horizontal alignment and combining cascade mode with
karaoke/highlight animation simultaneously (existing architectural
constraints, not needed by any theme in this set).

**A second real bug was found and fixed while verifying theme switching
actually showed up in playback**: `VideoPreview.jsx`'s live caption
overlay never applied any animation at all — the theme picker's own
preview cards animate (`classicPreviewAnimationClass`), but the main
video preview just rendered static styled text regardless of a theme's
`animation` field, making differently-animated themes look identical
during real playback. Fixed with real per-chunk one-shot entrance
animations (fade/pop/bounce/zoom/slide, timed to match the actual ASS
burn durations, retriggered via a `key` on the active caption chunk) and
genuine per-word rendering for karaoke/highlight (gray → theme color,
switching at each word's real timestamp) and typewriter (per-character
reveal) — using the real word-timing data already available, not an
approximation. Caught a second bug while verifying *that* fix live: the
per-word `<span>`s used `display: inline-block`, which silently collapsed
the space between words in the rendered page (confirmed via a real DOM
inspection — the text content correctly read "This ", but it rendered as
"Thisisarealtest") — fixed by removing the unneeded `inline-block`.

### Per-user default style, persisted in the database

Any style change — picking a different theme card, or hand-tweaking a
single field — is auto-saved (debounced ~1s) as the signed-in user's
default caption style, in a new small `user_settings` key/value table in
`library.db` (`library.rs`'s `get_default_caption_style`/
`save_default_caption_style`/`reset_default_caption_style`), keyed by
Firebase Auth's `user.uid` — deliberately per-user, not one global row,
since a tweak made by one signed-in user on a shared machine shouldn't
silently become another user's default. The next new project this user
creates starts from that saved default instead of the hardcoded factory
default (Cascade Bold); an already-open project's own already-loaded
style is never retroactively swapped out from under the user.

The style editor shows "Current: `<Theme Name>`" when the live style
still exactly matches a stock preset, or "Current: `<Theme Name>
(Custom)`" the moment anything has been hand-tweaked away from it —
computed fresh every render by comparing the current style against
`captionThemeId` (a new project-slice field, set only when a theme *card*
is clicked, never on a granular tweak) — deliberately not a separately
stored flag, so the label can never drift out of sync with the actual
style values. "Reset to factory default" reverts the currently open
project back to Cascade Bold immediately and clears the saved default
server-side, so future new projects also go back to the factory default.

## 2.11. Per-portion caption style overrides

Beyond one whole-video `CaptionStyle`, a project can have any number of
non-overlapping time-range overrides — e.g. a cold open in "Beast Mode,"
the rest in "Clean Classic" — layered on top of the base style. Two ways
to start one, both opening the same panel: **drag directly on the
Timeline's waveform** (a plain click still just seeks, unchanged — a
small pixel-movement threshold tells the two apart; the dragged range
snaps to the nearest word boundary), or click **"+ Add a style override
for this portion"** below the Timeline, which defaults to a 3-second
window starting at the current playhead position. Either way, both
entry points only ever produce an *approximate* range, so the resulting
`CaptionOverridePanel` shows the start/end as real editable number
inputs (seconds), not a read-only label — reuses `CaptionStyleEditor`
wholesale (full theme grid, category filter, every fine-tuning control)
seeded from the project's *current* base style, plus the same
overlap-check/Apply/Cancel either way. Existing overrides render as
bands on the Timeline and list in `MoreOptionsModal`'s style tab with a
Remove button each.

**The core mechanism — a range-aware style timeline**, implemented twice
in lockstep (once in `captions.rs`, used by both burn paths; once in
`src/lib/captions.js`, used by the live preview, so it can never show a
different chunk boundary or style than what actually burns): sort
overrides by `start`; walk them, filling every gap (before/between/after)
with the base style, each override becoming its own named piece. Each
piece's own word slice is then chunked *independently* with that piece's
own `words_per_line` — an override changing `words_per_line` or
`style_mode` (cascade vs. classic) genuinely changes how its portion
breaks into caption chunks, not just its color/font, which is what "apply
a different theme" actually has to mean. With zero overrides this reduces
to exactly the single global chunking pass this pipeline always did —
verified with an explicit regression test, not just assumed.

Non-overlap is enforced by the UI at creation time (`overrideRangeOverlaps`
in `src/lib/captions.js`, checked by `CaptionOverridePanel`'s Apply
button) — the style-timeline resolution itself never has to arbitrate a
tie between two overrides covering the same instant.

**Segmented (long-video, ≥ ~40s) burns**: each parallel-encoded segment
already calls `build_ass_document` independently with its own
already-rebased word list; `segments.rs` gained `shift_overrides` —
mirroring the existing `shift_speakers`'s overlap-filter-then-clip shape
(not `shift_prosody`'s simpler start-only filter), since an override, like
a diarization speaker segment, is a *range* that can straddle a segment
cut point chosen by `snap_to_gap` and must be clipped into every segment
it actually overlaps, never dropped or duplicated in full. Verified with
a real burn deliberately straddling a segment boundary.

**Verified for real**: burned a real test clip with three `Dialogue:`
lines referencing two different named ASS styles (matching
`build_ass_document`'s exact new output shape) and inspected the actual
rendered frames before, during, and after the override — confirmed
libass genuinely switches styles mid-video and reverts correctly, not
just assumed from the generated ASS text.

## 2.12. Live preview font-size scaling was wrong for vertical video (fixed)

`VideoPreview.jsx` scaled caption font size for its live overlay by
`displayWidth / 1920` — assuming the ASS script's `PlayResX=1920/
PlayResY=1080` (16:9 landscape) coordinate space maps onto the video by
*width*. Reels are vertical (9:16), so this assumption was wrong for the
app's actual dominant use case, and the effect was severe, not cosmetic:
burned two real test videos through libass with an identical style —
one portrait 1080x1920, one ultra-wide 3840x1080 — and measured the
actual rendered glyph height in the output frames. Both matched
`video_height / PlayResY` (1080), **not** `video_width / PlayResX`
(1920), regardless of which axis was larger than PlayRes. For a
realistic vertical reel shown in the app's small preview column, the old
formula produced an on-screen font roughly **3.16x too small** — small
enough that captions needing to wrap in the real burned output usually
just fit on one line in the live preview, which is exactly the report
that led here ("the downloaded video wrapped correctly, the live preview
didn't").

Fixed by scaling against the video's own rendered on-screen *height*
(`ASS_PLAY_RES_HEIGHT = 1080`) instead of width — the video element's
native resolution cancels out of the math entirely this way (`scale =
onscreen pixel height ÷ 1080`), so it's correct for any aspect ratio,
not just vertical, without needing to know the source video's real
pixel dimensions in JS at all.

**A smaller, separate gap, flagged rather than silently left unfixed**:
even at the now-correct font size, the exact word where a line breaks
can still differ slightly between the live preview and the real burn —
the browser's own CSS wrapping greedily fills each line to its max
width, while the real burn's ASS `WrapStyle: 0` does "smart" wrapping
that tries to balance line lengths (often keeping the top line shorter).
Verified directly: burning the same long sentence on a real vertical
video and comparing it (downscaled to the exact on-screen preview size)
against the corrected CSS preview showed clearly comparable,
proportionally-sized multi-line wrapping — a world away from the old
single-line overflow — but not an identical line-by-line split. Closing
that fully would mean re-implementing libass's specific balanced-wrap
algorithm in JS (using measured text width, not just CSS's default
fill-greedily behavior); not done here, since the font-size fix already
resolves the reported issue's actual root cause.

**Follow-up, same root cause, a second trigger**: the fix above still
wasn't enough for the preview's own fullscreen button specifically —
entering/exiting fullscreen changes the video's real rendered height
dramatically (a ~240px-tall preview column vs. the full display), but
nothing re-measured it there. The one `useEffect` re-measuring
`clientHeight` only listens for `window resize`, and fullscreening *an
element* via the Fullscreen API (rather than the whole browser window)
doesn't reliably fire that event — so `displayHeight`, and therefore the
caption font size, stayed stuck at the small preview's value even once
blown up to fullscreen, reproducing the exact same "barely wraps because
the font renders too small" symptom the height-based scale fix above
was meant to close, just via a resize path nothing was listening on.
Fixed by also re-measuring inside the existing `fullscreenchange`
listener, after a short delay to let the transition's layout actually
settle (reading `clientHeight` in the same tick the event fires can
still reflect the pre-transition size in some browsers).

## 2.13. Theme visibility/persistence UX + pin-and-stretch overrides

Three related UX improvements, all in service of the same complaint:
picking a theme (or an override) had no persistent, easy-to-find visual
confirmation of what was actually applied.

- **Selected-theme indicator on the card itself**: `CaptionStyleEditor.jsx`'s
  theme grid now highlights whichever card matches the project's current
  `captionThemeId` (a blue ring + checkmark), persisting across switching
  projects and coming back — previously the grid gave no visual signal
  at all about which theme (if any) a project was using; you had to
  compare every field by eye or trust the small text label below the
  grid. Matches by theme `id` alone, so it stays lit even once
  hand-tweaked into a "(Custom)" variant of that theme.
- **A second, always-visible location**: a new small panel just below
  the transcript box (`TranscriptPanel.jsx`), showing the current
  project's theme name (reusing `displayNameFor` — exported from
  `CaptionStyleEditor.jsx` for this — so there's one source of truth for
  the name, not two computations that could drift apart) and a "Change
  theme" link that opens the style editor directly (`moreOptionsOpen`
  was lifted from `MainPanel.jsx` up to `AppShell.jsx` so this sibling
  component can trigger the same modal). Since `MoreOptionsModal`
  remounts fresh each time it opens, it already always lands on the
  "style" tab by default — no extra plumbing needed for that part.
- **Pin-and-stretch for adding multiple per-portion overrides**: replaced
  the single continuous drag-to-select gesture with a two-step flow —
  click "📍 Pin a style override" (next to the Timeline's time label) to
  drop a pin at the current playhead, then drag anywhere on the timeline
  to stretch a range from that fixed point; releasing opens the same
  `CaptionOverridePanel` as before. A direct drag with no pin still works
  too, as a fast path. **Verified with real simulated mouse events**, not
  just read from the code: built a standalone test harness reproducing
  the exact state machine, confirmed a drag starting far from the pin's
  own position still produces a committed range anchored at the pin (not
  at the drag's own origin).

## 2.14. Multiple pins + auto-suggested transition points

Extends the pin-and-stretch flow above: any number of pins can be
dropped at once (clicking "📍 Pin a style override" repeatedly, moving
the playhead between clicks), but only one is ever *armed* — the fixed
edge the next drag stretches from — so it's always unambiguous which
pin a drag affects even with several sitting on the timeline together.
Clicking an unarmed pin arms it; clicking the already-armed one removes
it. Applying or removing the armed pin only ever touches that one pin —
every other placed pin stays put for later.

**Auto-suggested pins**, `suggestTransitionPoints` in `src/lib/captions.js`,
combine four signals — all computed from data this app already has, no
new backend analysis needed:
- **Speaker changes** (diarization) — a different speaker starting to talk.
- **Silence/pause gaps** — reuses `VideoPreview.jsx`'s own established
  "meaningful gap" threshold (0.6s; ≥1.5s is flagged as a "Long pause")
  rather than inventing a second, inconsistent number for the same idea.
- **Vocal-emphasis jumps** (prosody) — a jump into "high" intensity from
  a lower one.
- **Speaking-pace shifts** (new, proposed as a fourth signal alongside
  the three above) — local words-per-second just before vs. just after
  each word, purely from existing timestamps; a sudden speedup/slowdown
  often marks a real tonal shift the other three signals can miss
  entirely if speaker, pauses, and vocal emphasis all stay flat through
  it. A known, accepted limitation: near the very end of a fast/slow run
  (or the transcript's own end), the trailing window has less data to
  compare against, which can occasionally mislabel the direction of a
  real pace change — a real, minor artifact of any windowed heuristic,
  not a bug, and it doesn't affect *that* a transition gets flagged there.

Candidates from different signals within 0.5s of each other are merged
into one suggestion combining their reasons — multiple signals agreeing
on roughly the same moment is treated as a stronger, higher-`confidence`
suggestion than any signal alone. Suggestions landing inside an
already-styled override are dropped, and the result is capped at 12 so a
long video's timeline doesn't get cluttered. Rendered as dashed, dimmer
pin markers, distinct from a placed-but-unarmed pin (solid, dimmer) and
the armed pin (solid, full strength); clicking one opens a small menu —
see 2.15 below for what that menu offers (this replaced an earlier,
simpler "click = promote to an armed pin" behavior).

**Verified for real**, not just read from the code: ran the actual
exported `suggestTransitionPoints` against a synthetic transcript with a
deliberate speaker-change + pause + emphasis + pace-shift all landing
near the same real moment, confirmed the four signals correctly merged
into one `confidence: 3` suggestion snapped to a real word boundary, and
confirmed a suggestion inside an existing override is correctly
suppressed.

## 2.15. One-click auto-apply, a style menu on every pin, and editing an
existing override's style

Three related upgrades to the pin/override workflow above, closing the
gap between "the system found a good transition point" and "the video
actually looks different there":

- **A theme is now suggested, not just a moment in time.**
  `autoThemeIdForSuggestion` (`src/lib/themes.js`) maps a suggestion's own
  reasons/speaker id to a specific bundled theme — emphasis → *Beast
  Mode*, a speaker change → one of four themes rotated by `speaker_id %
  4` (matching `captions.rs`'s own existing per-speaker color palette, so
  the visual language stays consistent between diarization coloring and
  theme choice), speeding up → *Hustle Energy*, slowing down →
  *Documentary Serif*, a pause → *Minimalist Line*, falling back to the
  app's factory default otherwise. `autoRangeForSuggestion` picks a
  matching end time — a default ~4s span, capped early if another
  suggestion sits closer than that so two auto-applied ranges never
  overlap.
- **Clicking any pin now opens a small menu** (`Timeline.jsx`'s new
  `.timeline-popover`) instead of immediately acting, since a suggested
  pin and a placed override pin now have more than one thing you might
  want to do with them:
  - A **suggested** pin's menu offers "✨ Auto-apply: `<theme name>`"
    (creates the override immediately, with the auto-picked theme and
    range, no further clicks), "🎨 Customize style" (opens
    `CaptionOverridePanel` directly, pre-filled with the same
    auto-computed range but the project's *base* style rather than an
    auto-picked theme, so you pick the look yourself before Applying),
    and "🚫 Remove this suggestion" (a suggestion the user doesn't want
    to act on can be dismissed — tracked as a `Set` of dismissed times in
    `Timeline.jsx`'s own state, filtered out of the list before it's
    rendered *or* considered by auto-range-capping, so a dismissed one
    doesn't quietly keep constraining a neighboring auto-apply either).
  - **A real bug found here, from direct user testing**: "Customize
    style" originally just armed a pin and closed the menu — the actual
    style-picker never opened until the user *also* dragged a range by
    hand from that armed pin, which looked from the outside like
    clicking "Customize style" did nothing at all. Fixed as described
    above — it's now a direct action like Auto-apply, not a two-step
    "arm, then remember to drag" flow.
  - An **existing override band**'s menu (new — these weren't clickable
    at all before) offers "🎨 Change style" and "✕ Remove", showing the
    override's own current theme name in the menu's header
    (`displayNameFor`, already used elsewhere in this app for the same
    purpose).
  - The menu is `position: fixed`, anchored from the clicked marker's own
    `getBoundingClientRect()` rather than a percentage-based offset
    inside the track — `.timeline-track` itself has `overflow: hidden`
    (needed so waveform/word content never spills out), which would clip
    an absolutely-positioned menu popping up above it; escaping to
    viewport coordinates avoids that entirely.
- **Any existing override's style can now be changed**, not just removed.
  `CaptionOverridePanel` (previously create-only) now doubles as an edit
  panel: passing an override's own style/theme as the seed (instead of
  the project's base style) and its own index as `excludeIndex` (so the
  non-overlap check doesn't flag the range against itself) reuses the
  exact same component for both flows, rather than building a second
  one. `App.jsx` gained `updateCaptionStyleOverride(index, override)`
  alongside the existing `add`/`remove` pair; edit mode also renders a
  "Remove override" action so both changes and deletion happen from the
  same place a click on the pin opened.

**Not (re-)verified live in a running app for this change**: the app's
own dev server was occupied by the user's own running instance both
times this section's code was touched (including the "Customize style"
fix above, which the user themselves caught by hand — this app's own
usual real-browser click-through wasn't available either time), so this
was checked via `npm run build` / `cargo build --lib` (both clean) plus a
careful manual trace of the prop-wiring and of every export this depends
on (`autoThemeIdForSuggestion`/`autoRangeForSuggestion`/`displayNameFor`,
all already verified for real in earlier sections) — worth a real
click-through once the dev server is free, and worth treating this
section's UI with slightly less confidence than the rest of this
document until then, precisely because that's how the first bug here was
actually found.

## 2.16. Real video transitions (zoom punch / flash cut) at a pin

The pin/override system above only ever changed how *captions* look. This
adds a second, independent kind of pin action: a real effect burned into
the *footage itself* at that exact moment.

**A real architectural constraint, established before writing any code**:
this app's pipeline handles one continuous video per project — there's no
second clip to crossfade *from*, so a traditional multi-clip crossfade
transition doesn't apply here. What's built instead is two single-clip
effects, both common in short-form editing at a tone/topic shift:
- **Zoom punch** — a brief (0.4s) zoom-in-then-settle via ffmpeg's
  `scale`/`crop`, the scale factor a half-sine function of the frame's own
  timestamp (`t`), `eval=frame`.
- **Flash cut** — a short white flash: a solid-color clip, alpha-faded in
  (0.05s), held (0.05s), faded back out (0.05s), composited on top via
  `overlay`.

**Verified against real ffmpeg renders before any application code was
written** (`video_transitions.rs`'s own doc comment has the full detail):
- A synthetic 6s/180-frame test video confirmed both effects preserve
  the *exact* frame count and duration — critical, since these filters
  sit inside the same burn as every existing word/jump-cut/caption-override
  timestamp; any duration drift here would desync all of them.
- ffmpeg's `overlay` filter was confirmed to default to holding the flash
  clip's last (fully faded-out, transparent) frame once that clip ends,
  rather than truncating the whole output early — checked directly with a
  deliberately too-short 1s flash source composited onto a 6s video,
  confirming the output stayed the full 6s. This matters because the
  color source's `duration` is set from this app's own best-effort
  duration probe, which can occasionally come back `None`.
- **The full combined graph** (transitions chained into the *same*
  `-filter_complex` as the ASS caption overlay, transitions applied
  first) was rendered end to end with real burned-in text, confirming
  captions stay crisp, fixed, and unaffected by the footage zooming or
  flashing underneath them — the actual visual goal, and the reason
  transitions are applied *before* the `ass` filter in the chain rather
  than after.

**Backend** (`src-tauri/src/video_transitions.rs`, new): `VideoTransition
{ time, effect }`, `effect` one of `zoom-punch` | `flash-cut`.
`build_transition_filter` nests one `if(between(t,...))` per zoom (so
multiple zoom pins on one video don't interfere) and chains one
fade-in/fade-out pair per flash onto a single shared white color source.
`captions.rs::burn_captions` gained a `video_transitions` parameter:
when non-empty, it probes the video's real width/height/fps (new
`ffmpeg::probe_video_dimensions` — unlike ASS, which scales via a fixed
`PlayResX`/`PlayResY` regardless of actual resolution, these are literal
pixel filters and need the real numbers), builds the combined
transition+ass `-filter_complex`, and switches from `-vf` to explicit
`-map` for both video and audio (`-filter_complex` disables ffmpeg's
automatic stream selection entirely). **Not yet supported**: `segments.rs`'s
long-video parallel-encode path — a transition's absolute timestamp has
no shift/clip treatment across a segment boundary the way
`CaptionStyleOverride` already has via `shift_overrides`, so
`burn_captions` forces the single-pass path whenever any transition is
present, correctness over that path's speedup, matching the same
trade-off already made for a voiceover.

**Frontend**: `videoTransitions` is a new per-project array (`{time,
effect}`), parallel to `captionStyleOverrides` but deliberately
independent — a transition changes the footage, not caption style, so
either can be added to the same pin or to entirely different ones.
Reachable from three places in `Timeline.jsx`'s popover menus (a
suggested pin, an existing caption-style override, or a plain manually-
dropped pin all now offer "🎬 Add zoom-punch here" / "🎬 Add flash-cut
here" alongside their existing actions), rendered as their own small
amber markers in a dedicated overlay row (distinct from the full-height
pin/override lines so the two concepts stay visually separable even at
the same timestamp), each clickable for its own "✕ Remove transition"
menu — also listed, with the same remove action, in "More options → Style
captions" below the caption-override list. Cleared on a jump-cut
re-apply, same staleness reasoning as `captionStyleOverrides` (a re-cut
renumbers the whole timeline; a transition's absolute `time` would land
on whatever now happens to sit at that timestamp, not the moment it was
actually placed for).

Placing a plain dropped pin also changed here: clicking an *unarmed* one
used to arm it immediately; it now opens the same kind of menu a
suggestion/override gets ("✂️ Drag to set a caption-style range" plus the
two transition actions plus remove) — clicking the *already-armed* one
still removes it directly, unchanged, as a quick mid-drag-prep shortcut.

**Not verified live in a running app**: same real constraint as 2.15 —
the dev server was occupied by the user's own running instance while this
was built. Checked via `npm run build` / `cargo build --lib` (both
clean, `cargo test` itself still blocked by this environment's
pre-existing `STATUS_ENTRYPOINT_NOT_FOUND` DLL issue, unrelated to this
change) plus the real ffmpeg renders described above, which cover the
part of this feature with genuine technical risk (the filter graph
itself); the UI wiring is comparatively low-risk prop-threading, worth a
real click-through once the dev server is free.

### 2.16.1. Three real bugs found from actual use, plus a live-preview approximation

All found by the user actually clicking through the feature above, not
caught by the build/render verification alone:

- **The popover could render off-screen with no way to reach the hidden
  buttons.** The menu always drew *above* the clicked marker; with the
  timeline sitting near the top of the window, its own top rows (the most
  important ones — "Auto-apply"/"Change style"/"Customize") rendered
  above the visible viewport and were silently clipped by the browser's
  edge, invisible with no indication anything was missing. A first fix
  (estimate the menu's height, flip below if the marker looked "too close
  to the top") wasn't enough — a short window, or a marker near a
  *different* edge, could still clip it. Replaced with a proper
  measure-then-position approach: `Timeline.jsx` renders the popover
  hidden (`visibility: hidden`, off-screen) first, a `useLayoutEffect`
  measures its *real* rendered `offsetWidth`/`offsetHeight` (which varies
  with content — a suggestion's menu has more rows than a transition
  marker's), then computes `top`/`left` clamped fully inside
  `window.innerWidth`/`innerHeight` on every edge before making it
  visible — no more guessing.
- **"Pin a style override" had no way to reach the new menu at all.**
  That button auto-*armed* the pin it dropped (a leftover from before the
  popover existed — "drop pin, immediately ready to drag"), skipping the
  unarmed state a click would normally open a menu from. Clicking the new
  pin again (since it was already armed) just removed it via the
  "click-the-armed-pin" shortcut — there was no path left to the menu at
  all for a pin dropped this way. Fixed: the button now opens the same
  menu any other pin does, computed from the timeline track's own
  bounding rect and the playhead's position (there's no click event to
  anchor to, since the pin doesn't exist in the DOM yet) — its first
  option, "✂️ Drag to set a caption-style range," now covers what the
  auto-arm used to do, as an explicit choice instead of the only option.
- **The override list (More options → Style captions) showed a
  hand-tweaked override's *base* theme name with no indication it had
  been customized** — a raw `CAPTION_THEMES` lookup by id, not the
  already-existing `displayNameFor` helper (which appends "(Custom)"
  once any field diverges from the theme's stock style) that
  `Timeline.jsx`'s own tooltip/popover for the same override already
  used. Fixed to call the same helper, so the two places agree.
- **Neither effect showed up in the live preview at all** — a real,
  honest gap, not a bug: `VideoPreview.jsx` plays the original video file
  directly via a plain `<video>` element and only overlays caption text
  on top; it never re-renders pixels the way `burn_captions` does, so
  there was nothing there to make a zoom or flash happen. Added a CSS-only
  *approximation*: a `transform: scale()` animation applied directly to
  the `<video>` element for zoom-punch (clipped back down to
  `.video-frame`'s own bounds via a new `overflow: hidden` on that
  element, mirroring the real crop-back-to-original-size shape) and a
  plain white `opacity`-animated div for flash-cut, composited as a
  sibling *before* the caption overlay layer in the DOM so captions still
  paint on top, matching the real burn's order. Both effects use two
  identically-shaped keyframe sets per effect under different names
  (`-a`/`-b`), alternated on each trigger — re-applying the *same*
  animation class while one is already playing doesn't restart it in CSS,
  since the resolved `animation-name` value wouldn't actually change;
  alternating guarantees a genuine change even for two same-effect
  transitions placed close together. Explicitly a preview
  *approximation*, not a promise of frame-identical timing/shape to the
  real ffmpeg filter graph — said so directly in the code's own comments
  rather than implying it's pixel-equivalent.
  - **Verified for real** (not just read from the code): the crossing-
    detection algorithm (`t.time > lastChecked && t.time <= currentTime`,
    the rule that decides when to fire) was reproduced standalone and run
    under real Node.js assertions covering normal forward playback
    (fires exactly once), seeking backward past a transition (does not
    fire), seeking forward across one again (fires again, alternates
    variant), scrubbing forward past two same-effect transitions in one
    jump (both fire, variants end up correct), and two different effects
    at different times (independent counters, no cross-interference) —
    13 assertions, all passing.

### 2.16.2. Auto-picking which transition effect fits — a rule, not a model

Extends "✨ Auto-apply" (2.15) to also drop the better-fitting transition
effect at the same point, not just the caption theme. Deliberately **not**
a model of any kind, small or otherwise — `autoTransitionEffectForSuggestion`
(`src/lib/captions.js`) is a plain priority-ordered rule over the same
signals `suggestTransitionPoints` already computed (speaker change, pace,
pause, emphasis), the same shape as `autoThemeIdForSuggestion` already
uses for theme selection. Picking one of two fixed, categorical options
from already-structured signal data isn't an open-ended generation task
the way writing content-strategy copy or a music prompt is (the two
places this app *does* reach for its local LLM) — there's no training
data for "which transition effect is best," and a rule captures the same
editorial logic a model would have to learn anyway, for zero cost and
instant, deterministic results.

The editorial logic: a flash cut is the traditional hard-punctuation mark
between two different speakers or during a quiet beat — brief, low-energy.
A zoom punch rides *rising* energy — vocal emphasis or a pace speedup.
Slowing down doesn't fit either effect's own energy well, but only two
effects exist in the library today, so it takes the calmer of the two.
Clicking "✨ Auto-apply" now shows and applies both picks together (e.g.
"Auto-apply: Golden Karaoke + 🎬 Zoom punch") in one click; the manual
"Add zoom-punch/flash-cut here" buttons are unchanged for full control.

**Verified for real**: ran the actual exported function against every
individual reason plus priority-order cases (two reasons present at once,
in both array orders) — 9 assertions, all passing, confirming Emphasis
correctly outranks Speaker change regardless of which one
`suggestTransitionPoints` happened to list first.

## 2.16.3. Growing the transition library (shake, color pulse), and a
cloud-vs-local detour

Before adding effects, real research (not assumed) into what professional
editors and short-form platforms actually use, current as of this
writing:
- **Classic film editing**: cross dissolve, wipe, iris, whip pan, match
  cut — [StudioBinder's guide](https://www.studiobinder.com/blog/types-of-editing-transitions-in-film/),
  [Wedio's guide](https://www.wedio.com/en/learn/types-of-transitions-in-film).
  Nearly all of these are fundamentally **multi-clip** techniques (one
  shot replacing another) — they don't apply to this app's single-
  continuous-clip pipeline, the same real constraint already established
  when zoom-punch/flash-cut were first chosen over a true crossfade.
- **Current (2026) short-form editing**: velocity edits (speed ramps),
  glitch/RGB-split, smooth zoom-ins, whip pans, and music-locked cuts —
  [CapCut's own trend page](https://www.capcut.com/help/capcut-transitions).
  Effective ones run 0.2–0.4s and land on a beat or word — matching the
  durations already chosen for this library (0.15–0.4s) independently.

From that list, **velocity edits/speed ramps are excluded on purpose** —
any effect that changes local playback speed shifts every word timestamp
after it out of sync with captions, the same reason a true crossfade was
excluded earlier. Glitch/RGB-split is a strong future candidate (flagged,
not built this round). **Shake/jitter** and **color pulse** (both
requested) were added now:

- **Shake**: reuses zoom punch's own "enlarge slightly for headroom, then
  crop back down" trick, but the crop's x/y offset wobbles via two
  different-frequency sine/cosine terms (9Hz and 11Hz) instead of staying
  centered — different frequencies so it traces a genuine 2D jitter
  rather than one straight diagonal line back and forth.
- **Color pulse**: a brief desaturate-to-gray-and-back via ffmpeg's
  `eq=saturation=`, the same half-sine-bump shape zoom punch's scale
  factor already uses.

**Verified against real ffmpeg renders before any Rust code was
written**, same discipline as the first two effects: a real 6s/180-frame
test video confirmed both preserve the exact frame count and duration:
extracted consecutive frames across the shake window and confirmed the
offset genuinely oscillates (not just drifts in one direction — two
frames landed at different offsets consistent with two different phases
of the wobble, not further along a single line); extracted the
color-pulse's peak frame and confirmed a real, visible desaturation
(vibrant color bars visibly washed to muted gray/brown/green tones),
distinct from flash-cut's white wash.

**A real cloud-API question was raised and answered honestly, not
assumed**: whether a cloud video-generation API (Runway, Luma, Kling,
Veo, or a hosted-model marketplace like Replicate/fal.ai) could generate
transitions "effectively and economically." Conclusion: no, not for this
job specifically — those APIs charge per second of *generated* video
(real money, real network/generation latency, the user's footage leaving
their machine) for something ffmpeg already produces for $0, instantly,
entirely locally. This would also be the app's first cloud dependency in
its core editing pipeline, breaking a pattern held everywhere else (LLM,
STT, TTS, music generation, embeddings are all local). A genuinely
different, generative transition (not a parametric filter) would be a
real use case for cloud video generation, but that's a distinct, much
bigger feature needing its own real cost/quality investigation before
committing — not something to fold into this library.

Rust: `video_transitions.rs`'s `TransitionEffect` enum gained `Shake`/
`ColorPulse` variants; `build_transition_filter` now chains up to four
stages in a fixed order (zoom → shake → color pulse → flash), each
optional, computed once from real integration tests (`all_four_effects_
chain_in_fixed_zoom_shake_pulse_flash_order`, `single_shake_...`,
`single_color_pulse_...`) alongside the two carried over from before.

## 2.16.4. Collapsing four sprawling per-pin menus into one small,
consistent menu

Real user feedback: with the library at four effects, the old per-pin-
kind popovers (a suggestion's menu alone had grown to 6 rows: Auto-apply,
Customize style, four "Add `<effect>` here" buttons, Remove suggestion,
Cancel) had become unwieldy, and the request was explicit — keep it to
"add/edit/remove (if added) caption style, add/edit/remove (if added)
transition, remove pin, cancel."

This replaced four separate, hand-built popovers (one per pin "kind":
suggestion, override band, plain pin, transition marker) with **one**
menu built fresh from what's actually at the clicked time, regardless of
which kind of marker was clicked:
- `Timeline.jsx`'s `openPopoverAt(time, source, event)` resolves, once,
  at open time: whether a `captionStyleOverride` already covers this
  exact time (`overrideIndex`), whether a `videoTransition` already sits
  here (`transitionIndex`, matched within a small tolerance), and —
  only for a suggestion — the `reasons`/`speakerId` that let the menu
  seed a smart default.
- The menu then shows **Add** for whichever of caption style/transition
  is missing, or **Edit** + **Remove** for whichever is already present —
  the exact same menu shape no matter which of the four marker kinds was
  clicked, capped at a handful of rows regardless of how large the
  transition library ever grows.
- "Add transition" (or "Edit transition") opens a small second screen —
  the four effect choices plus a "Back" — rather than the top-level menu
  growing a row per library entry. The previously-separate "✨ Auto-apply"
  one-click action was folded into "🎨 Add caption style" instead of kept
  as a second option: it now always opens the style panel, but pre-fills
  it with the auto-picked theme (`autoThemeIdForSuggestion`) when the pin
  came from a suggestion — the "smart default" is preserved, just no
  longer a separate button next to "Customize."
- **A real dead-code cascade this simplification caused**: the old
  "arm a pin, then drag to stretch a range from it" workflow (an entire
  `armedIndex`/`armPin`/drag-anchor mechanism) had no menu action left
  that could ever set it once "✂️ Drag to set a caption-style range" was
  removed from the (now-unified) menu — CaptionOverridePanel's own
  editable numeric start/end fields already cover the same "adjust the
  range" need. Removed entirely rather than left as unreachable code:
  `armedIndex` state, `armPin()`, the CSS `.timeline-pin-marker.armed`
  rules, and the mousemove handler's armed-pin drag-anchor branch (the
  direct drag-on-the-waveform gesture itself is untouched — it never
  depended on pins at all).

"Remove pin" only appears for a suggestion or a manually-dropped pin —
an override band or transition marker has no separate "pin" entity of
its own to remove, only its own content (removing that already leaves
nothing behind).

## 2.16.5. LLM-refined transition planning ("✨ AI-suggest transitions")

The heuristic candidate list (`suggestTransitionPoints`) only ever reasons
from *timing* signals — a pause, a speaker change, a pace shift, vocal
emphasis. It has no idea what's actually being said, so it can't tell "a
mechanical pause mid-sentence" from "the exact moment a struggle turns
into a win." This adds a second pass, using the same local LLM already
running this app's content-strategy and music-suggestion features
(`llm.rs`, Qwen2.5-0.5B-Instruct), that reads the real transcript text
and decides which of the heuristic's own candidates actually deserve a
transition, and picks the best-fitting effect for each using the
transcript's own content — not just the mechanical reason code.

**A real cloud-vs-local question preceded this, answered honestly, not
assumed** (raised directly: "is there a cloud API to do this
economically"): no cloud video-generation API (Runway, Luma, Kling, Veo,
or a hosted-model marketplace) is worth it for this specific job — they
bill per second of *generated* video for something this app's own local
LLM (already running, already free) can plan from plain text in a
fraction of a second, and it would be this app's first cloud dependency
in its core editing pipeline, breaking a pattern held everywhere else.

**The design changed once, based on real evidence, not preference.** The
first design let the LLM freely propose its own transition timestamps
from a compact per-sentence JSON summary (start/end/text/speaker/
silence-before/after — the same shape ffmpeg scene-detection + Whisper
transcription would naturally produce). Tested directly against the real
local model across several synthetic transcripts before writing any Rust:
twice, it returned a `time` that didn't match any real sentence boundary
at all — once literally a segment's own *end* time instead of a start.
Free generation of a number that happens to be exactly right is a genuinely
harder constraint-following task for a 0.5B model than classifying among
a handful of numbers it's explicitly given. **Redesigned instead to only
ever let the LLM choose among the heuristic's own already-real candidate
times** — never invent one — which closed the failure mode structurally:
re-verified across 5 fresh test runs (3 different transcripts, 2 of them
exact repeats for a consistency check) with **zero invalid times**
returned. `transition_planner.rs`'s own `plan_transitions` still validates
every returned time against the real candidate list as a defensive
backstop regardless (plus deduplicates — one real test response repeated
the same time twice) — never trusting the model's echo blindly, even
after a clean verification run, the same "verify, then still validate"
discipline `content_ideas.rs`'s `sanitize_hashtag`/`sanitize_emoji` already
apply to a different untrusted-output failure mode.

**A second, real, honestly-kept limitation, not fixed**: asked to judge a
plain step-by-step list with no real narrative shift (a baking recipe, a
coding tutorial), this 0.5B model does not reliably say "none of these
deserve one" — even with a worked few-shot example demonstrating exactly
that correct answer, it still tended to keep at least one candidate with
an invented-sounding reason on this kind of flat content. Matches this
project's own established precedent for owning a small local model's real
limits (`content_ideas.rs`'s Tamil-comprehension gap, `music_gen.rs`'s
imperfect mood-matching) rather than silently shipping around them. This
is mitigated architecturally rather than solved: every result is still a
plain `VideoTransition`, individually removable with one click via
Timeline.jsx's own popover, never auto-committed to anything harder to
undo — and since it's only ever *classifying* the heuristic's own
already-pre-filtered candidates, a false positive here is never worse
than what the heuristic alone would already have suggested.

**Research into what professional editors/current platforms would call
these transition types** (dissolve, zoom-blur, whip-pan) also directly
informed why this doesn't use those names or ffmpeg's `xfade` filter
family: see 2.16.3's own research section — those are fundamentally
multi-clip crossfade techniques that would need to split this app's one
continuous video and re-derive every downstream timestamp, a separate,
larger investigation (still open — see "Next: a real crossfade
feasibility investigation" below), not something to fold into this pass.
The four effects this LLM plans among are exactly the ones that already
exist and render correctly today (zoom-punch, flash-cut, shake,
color-pulse) — the JSON-schema-constrained generation this app already
uses everywhere else (`content_ideas.rs`, `music_gen.rs`) keeps the model
from producing anything outside that enum at the token level, and the
response is deserialized straight into `video_transitions.rs`'s own real
`TransitionEffect` type, not a raw string.

Backend: new `src-tauri/src/transition_planner.rs`, one command
(`suggest_transition_plan`). Segments the transcript into sentence-like
chunks (breaking on `.`/`!`/`?`), computes real silence-before/after gaps
and speaker ids per segment (reusing `captions.rs`'s own `speaker_id_at`,
promoted to `pub(crate)`), budgets the transcript the same way
`content_ideas.rs`/`music_gen.rs` already do (`llm_budget.rs`, this
prompt's own real measured token cost — 1020 tokens wrapped, via
llama-server's `/tokenize` endpoint), and calls `llm::complete` at a low
0.2 temperature (a classification task over given candidates, not
creative generation, the same reasoning `tts.rs`'s emotion classifier
already documents for its own low temperature). Frontend: `App.jsx`'s
`suggestTransitionPlan` writes through `upsertProjectSlice(forProjectId,
...)` directly (not the `patchCurrentProject`-bound `addVideoTransition`)
since this is an async, several-second call — the same "never assume the
project you started this for is still on screen when it resolves" rule
`generateContentIdeas`/`runPipelineFor` already follow. A new "✨
AI-suggest transitions" button next to Timeline.jsx's existing "📍 Pin a
style override" sends the current (non-dismissed) suggestion list's own
times and reports back what it added, including the LLM's own authored
reason for each (shown once in a dismissable message, not persisted on
the `VideoTransition` itself — that struct is also `burn_captions`'s own
input shape, and the reason has no bearing on the actual ffmpeg filter).

**Next: a real crossfade feasibility investigation** (not started) — the
open question is whether it's worth teaching the whole downstream
pipeline (captions, jump cuts, other overrides/transitions) to re-derive
their timestamps after a mid-video crossfade shortens the video by the
overlap duration, which is what would actually be needed to offer a real
dissolve/wipe effect (via ffmpeg's own `xfade` filter, splitting the
video at the transition point) rather than the four same-duration-in/out
effects this pass shipped.

## 2.17. A clean, minimal sidebar: edit-via-pencil, fast full-text search,
project status, view-original relocated

Requested directly, several real changes to the project library sidebar
(`Sidebar.jsx`) and its backend (`library.rs`):

**Editing moved behind a pencil icon.** Title/description/hashtags used
to be permanently-visible inline inputs at the top of the sidebar,
occupying space and staying editable whether or not anyone was actually
using them. They're now read-only display text; clicking the new ✏️ icon
next to the current project's title opens `EditProjectModal.jsx` (new,
`.shell-modal` convention, same as `MoreOptionsModal.jsx`) with a real
Cancel that discards unsaved changes, since a modal needs that in a way a
permanently-open field never did. AI-generated title/hashtags still
auto-fill the moment transcription finishes, exactly as before -- that
writes into the same store fields this now just displays.

**"View original video" moved to an icon next to the pencil**, replacing
the old always-visible thumbnail box + text link below it. Toggling it
still shows the same real, untouched original file (`originalVideoPath`,
set once per project and never mutated) inline, just triggered from the
title row instead of a separate large box.

**Semantic (embedding-based) search replaced with SQLite's own FTS5 full-
text search** -- requested directly ("fetch results faster," "ignore
vector embeddings"). Real, concrete wins, not just a simplification:
- **No more warm-up latency.** The old version needed a Python
  `sentence-transformers` server (`embeddings.rs`, now deleted) kept
  running in the background, with a real ~19s model-load the first time
  a fresh app session searched. FTS5 is built into SQLite itself --
  `search_projects` now runs synchronously, in-process, with no server to
  start at all.
- **No more separate re-embed step.** The embedding version needed its
  own debounced "re-embed this project" timer (`projectStore.js`) to keep
  the search index from drifting out of sync with edits. FTS5's index is
  now rebuilt synchronously, in the same function, as part of the exact
  `insert_project`/`update_project_row` calls that already run on every
  save -- one less moving part, not a new one.
- **Verified directly before writing any code**, matching the discipline
  the original `sqlite-vec` integration itself used: a standalone probe
  confirmed FTS5 needs no extra Cargo feature -- it's already compiled
  into this project's exact `rusqlite = { version = "0.32", features =
  ["bundled"] }` -- and confirmed the query-sanitization scheme this uses
  (wrap every whitespace-split term in escaped double quotes plus a
  trailing `*`, e.g. `foo bar` -> `"foo"* "bar"*`) survives every FTS5
  special-syntax character tried against it (an apostrophe, a hyphen, a
  bare `"`) without erroring, while still supporting real prefix matches
  (typing "gar" finds both "garlic" and "Garage"). `sqlite-vec`/
  `zerocopy` were removed from `Cargo.toml` entirely, not just left
  unused, once nothing called into either anymore.

**Real project status, shown as a badge next to the title** (both the
current project's header and every row in the list): Pending → 📝
Transcribed → 🔥 Burned → 📤 Exported → ✅ Published, plus a live ⚙️
Processing that overrides all of them while a background job is actually
running. Computed entirely in a new `src/lib/projectStatus.js`, never in
Rust -- `Project.state` is deliberately opaque to `library.rs` (see that
module's own doc comment on why), so anything that needs to peek inside
it to derive a status belongs on the same side that already assembles and
reads that blob. Two of these needed a small, honest backend addition
first: `exported_at`/`published_at` are new columns, stamped by new
`mark_project_exported`/`mark_project_published` commands the moment
`ExportButton.jsx`'s Download and `ScheduleToInstagramButton.jsx`'s
"post now" actually succeed -- distinct from merely having burned a file,
which `last_burned_path` already covered. **One real, open gap, flagged
rather than faked**: a *scheduled* (not-yet-fired) Instagram post never
marks a project published, since `scheduler.rs`'s own queue has no notion
of which project a scheduled job came from -- only an immediate "post
now" is tracked. Making the scheduled path work the same way would mean
teaching the scheduler's own persisted job format about project ids, a
real, separate change not folded into this pass.

## 2.18. Per-row actions, a popup video preview, and a "+" flow with an
AI content-strategy intake form

A second sidebar pass, requested directly, on top of 2.17's redesign —
planned in full first (see `EnterPlanMode`'s own approved plan) since it
touched enough surface area and had enough real forks (a real second OS
window vs. an in-app modal for video preview; a silent on-disk draft vs.
a real, visible library entry) to be worth confirming before writing any
code, rather than guessing.

**Every row gets its own ✏️/🎬 actions now**, not just the currently-open
project. `EditProjectModal.jsx` (already built for the single "currently
open" case) turned out to need no changes at all to support this — it
was already generic over `title`/`description`/`hashtags` + `onSave`;
only `Sidebar.jsx`'s own call site needed a second save path (`invoke("
save_project", ...)` directly, for a row that isn't the live/currently-
open project, vs. the existing live setters when it is). Video preview
moved from an inline thumbnail/player into `VideoPreviewModal.jsx` (new,
plain `.shell-modal` convention, confirmed directly as the right call
over a real second OS window like the dictation HUD — consistency with
every other popup in this app over a more "native" but heavier build) —
every row already carries its own `original_path` from `list_projects`,
so this has no dependency on whichever project happens to be open.

**The whole standalone "Media Pool" section is gone.** No more permanent
video thumbnail, "Choose video(s)…" button, or separate "currently open
project" mini-header at all — the active row's own highlight is the only
"what's open" indicator now. In its place: a small "+" icon next to the
"Projects" heading, opening `NewProjectModal.jsx`.

**That popup has two tabs.** "Upload video" is a thin wrapper around the
exact same multi-file `pickVideo` flow that already existed. "✨ AI
content strategy" is new: a from-scratch video-generation intake form —
**parameters only, actual generation explicitly deferred** to a future
cloud-API integration (this project's own earlier local-model
investigation already found real generation wasn't CPU-feasible at a
usable speed — see the paused plan's real measured numbers above; this
picks the idea back up on the input side only). Asked directly to
"suggest more parameters" for an efficient pipeline, the form ended up
with five groups, reusing this app's own existing concepts where one
already fit rather than inventing parallel ones (spoken language;
`content_ideas.rs`'s own free-text `hints`; `music_gen.rs`'s own
real 3-category music framing, chosen originally because a plain "pick a
mood" ask collapsed into near-duplicate suggestions at this model's
size):
- **Core brief**: project name, content brief, goal/call-to-action.
- **Audience & market**: target country, target audience, industry/
  sector, culture, spoken language.
- **Character & visual style**: character description *and* an optional
  reference photo (not either/or), visual style/mood, brand colors.
- **Tone & format**: tone, target length, music mood.
- **Extra hints** (optional freeform).

**Submitting it creates a real, visible project** — confirmed directly,
not assumed, as the right call over a silent on-disk file: a new
"📋 Drafted" status (`projectStatus.js`, checked right after "processing"
and before every other status, since a draft with no video can't be
transcribed/burned/exported/published regardless of what else its row
says) so there's something real in the list to come back to once
generation exists, not an easy-to-forget file. Backend: one new
`library.rs` command, `create_strategy_draft`, deliberately handling
*both* create and update (`id: None`/`id: Some(existing)`) rather than a
separate edit path — a real correctness reason, not just less code: only
routing both through the same character-photo-copy step keeps a
*changed* reference photo from leaking its raw, pre-import OS path into
`state` unconverted the way a naive direct `save_project` call on the
edit path would have. `original_path`/`original_filename` are stored as
plain empty strings for a draft — both columns are already `NOT NULL`
but nothing else in the schema requires them non-empty, and
`MainPanel.jsx` already had a real "no video" empty state for a falsy
`videoPath` before this — `projectStatus.js` is what turns that into a
real "Drafted" status instead of a broken-looking blank project.
Clicking a Drafted row (or its own pencil icon) reopens
`NewProjectModal.jsx` straight to the strategy tab, pre-filled, calling
that same upsert command again with its own id.

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

**Slim installer (default).** Bundles only the app + fonts; the runtime
(ffmpeg, the `stt`/voice Python envs, the LLM) is fetched on first launch
/ first use by `runtime_fetch.rs`. Needs no populated resource dirs.
```bash
npm run build:slim
```
Output: `src-tauri/target/release/bundle/msi/KraftReel.App_0.2.0_x64_en-US.msi`
(~6 MB, measured). `release.yml` builds this per OS (`.msi` / `.dmg` /
`.deb` + `.AppImage`). The app is inert until `components.json`'s asset
URLs/checksums are published by the pipeline — Windows packs first;
macOS/Linux packs slot into the same manifest by platform (see
`docs/RUNTIME-PACKS.md`).

**Full installer (offline).** Bundles the whole runtime, no first-run
download. Populate the resource dirs first:
```bash
npm run fetch-resources
node scripts/build-python-runtime.mjs --pack=all
npm run build:full
```
Output: the same path, ~2–3 GB. Ship both from CI — slim for everyone,
full for air-gapped users.
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
(`windows-latest`, `macos-latest`, `ubuntu-22.04`), each running `npm run
tauri build` natively and uploading its own installers as release assets.
**Set up as `.github/workflows/release.yml`**, triggered by pushing a
version tag (`git push origin v0.2.0`) — see section 9.2's "Cutting a
signed release" for the full flow, including the one-time repo secrets it
needs for signing.

### What a fresh install actually gets you

The installer bundles ffmpeg/ffprobe, the local LLM (llama.cpp +
Qwen2.5-0.5B-Instruct), Piper's English voice, and the Tamil font —
**English caption styling/burning, silence removal, English voiceover, and
title/hashtag generation all work immediately after install, no setup.**

**Tamil transcription and voice cloning** need one more model download
(the Tamil Whisper checkpoint + the OpenVoice converter checkpoint, ~360MB
combined) that's deliberately not baked into the installer — see "In-app
model download" below, which handles this with one click, no terminal
required. **Every conda environment (`stt`/`tts`/`media-ai`/
`voice-clone`) is still a manual, per-machine setup step** (sections 2,
2.4, 2.6, 2.7) regardless of that download — those need real Python
packages installed (torch, transformers, faster-whisper, openvoice...),
a meaningfully bigger scope (bundling a whole Python distribution) this
project has deliberately kept out of the installer. Worth saying plainly
to anyone you hand this to: **installing the MSI alone does not give you
working transcription of any language** — mention the conda setup steps,
or they'll hit "STT engine not found" the moment they upload a video.

### In-app model download (Tamil transcription + voice cloning)

`model_fetch.rs` checks, on app load, whether the Tamil Whisper checkpoint
and the OpenVoice converter checkpoint already exist under
`~/.reels-caption-app/` — the same per-machine fallback location
`stt::indic_model_dir`/`voice_clone::voice_clone_checkpoint_dir` already
check as their last resort. If either is missing, `shell/
OptionalModelsBanner.jsx` shows a small dismissible prompt (bottom-left,
mirroring Burn & Export's bottom-right dock) with a "Download now" button.

This fetches a **separate, smaller release asset**
(`optional-models.tar.gz`, ~335MB) from the same GitHub Release as the
installer itself — deliberately not the ~1.1GB `dev-resources.tar.gz`
developers use (section 3.1), which also re-bundles ffmpeg/llama/Piper
that an installed app already has; re-downloading those here would be
pure waste. Extraction shells out to the system `tar` (bundled with
Windows 10+/macOS/Linux), same reasoning as `fetch-dev-resources.mjs`.

**Why the installer itself doesn't do this download** (a real option that
was considered): MSI's transactional install model handles long network
operations poorly — a stalled multi-hundred-MB download partway through
can leave the whole install in an awkward rollback state, it often runs
without the user's normal network/proxy context, and antivirus/corporate
policies commonly flag installers that reach the network mid-install. A
first-run in-app download avoids all of that: the app launches
immediately regardless of network state, and the download reuses the same
progress-event pattern already used for transcription/burning elsewhere
in this app.

**Publishing a new release**: keep `optional-models.tar.gz` and
`dev-resources.tar.gz` both up to date as separate assets — `gh release
upload <tag> optional-models.tar.gz dev-resources.tar.gz --clobber`. The
in-app downloader always fetches from `.../releases/latest/download/...`,
so it automatically picks up whatever the newest release's asset is —
no version string to update in code.

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

- **Speech-to-text** (`pipeline::transcribe_with_auto_language`, which
  delegates to `mixed_language::transcribe_with_language_detection`):
  after extracting audio, the spoken language is detected automatically
  and routed to whichever model matches — Parakeet for English, the Tamil
  checkpoint for Tamil — the same way the old dropdown did manually. A
  detected language with no matching model is a clear error naming what
  was detected and what's supported (`stt::supported_language_names`), not
  a silent misroute or a confusing downstream crash. This also
  automatically handles a video where different stretches are in
  different supported languages (e.g. one person in Tamil, another in
  English) — see section 7.4 for how. Used by both `run_pipeline` (the
  main clip) and `transcribe_audio_file` (re-transcribing a voiceover).
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

### 7.3. Tanglish slang normalization (optional)

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

### 7.4. Mixed-language videos (code-switching)

No separate mode to pick, no checkbox — every video goes through the same
segment-and-classify detection (`mixed_language.rs`) described here, which
handles a video where different speakers (or the same speaker
mid-sentence) use different supported languages, e.g. one person in Tamil,
another in English, exactly as well as an ordinary single-language video.
An earlier version of this feature was opt-in (a sidebar checkbox,
"costs meaningfully more, don't run it for videos that don't need it") —
removed once it became clear that detecting *reliably* is worth doing
unconditionally: the old single-call whole-file `detect_spoken_language`
this replaced had its own blind spot even for a plain single-language
long-form video (Whisper's language-ID only examines roughly the first
~30 seconds of whatever audio it's handed, silently ignoring the rest —
see step 1's `MAX_SEGMENT_SECONDS_FOR_LANGUAGE_ID`), and a genuinely
single-language video still collapses to exactly one chunk below, taking
the same fast one-call transcription path as before with no slicing
overhead at all.

How it works, in order:
1. **Segment first, language-agnostically** — `ffmpeg::detect_speech_segments`
   runs ffmpeg's own `silencedetect` filter directly on the waveform to
   find speech-vs-silence boundaries. Deliberately *not* derived from
   word-level timestamps the way `jumpcuts.rs`'s silence-gap logic is:
   getting word timestamps first would require already transcribing the
   file in *some* one language, the exact chicken-and-egg problem this
   avoids.
   Any single segment longer than `MAX_SEGMENT_SECONDS_FOR_LANGUAGE_ID`
   (8s) gets further subdivided into equal sub-windows purely for the
   language-classification step below — Whisper's own language-ID only
   ever examines roughly its first ~30 seconds of input, so without this a
   long, pause-free stretch (confirmed on a real ~48-second continuous
   take with no gap ≥0.5s anywhere) would get classified from only its
   opening few seconds, silently ignoring everything after. The
   same-language merge step (3, below) still recombines these sub-windows
   into one contiguous transcription chunk afterward, so this doesn't
   fragment the final transcription — only how finely language is
   *sampled* across a long stretch.
2. **Classify each segment's language** — `stt::detect_spoken_languages_batch`
   runs a small `Systran/faster-whisper-tiny` detector, loaded once and run
   over every segment in one process (`stt/detect_language_segments.py`)
   instead of paying full model-load time per segment.
3. **Merge for quality, not just correctness** — classifying and
   transcribing every raw silence-bounded segment independently would
   fragment the job into a lot of short clips, and less audio context per
   STT call measurably hurts accuracy. So a segment too uncertain to trust
   on its own inherits the nearest already-resolved neighbor's language
   (checking both directions — a leading untrusted segment with no prior
   neighbor yet still gets filled in from the next trusted one, rather than
   being left stuck on a bogus guess) instead of forcing a split, and
   consecutive segments that resolve to the same language are merged into
   one contiguous chunk before transcribing. "Too uncertain to trust" is
   confidence-led, not duration-led — a short but unambiguous detection
   (this project's own benchmark: 98.7% on English, 91-95% on Tamil) is
   trusted at any length past a tiny 0.35s floor; a lower-confidence one
   needs at least 0.8s to stand on its own. Getting this backwards (originally
   requiring both ≥1.2s duration *and* ≥55% confidence) was a real bug found
   via a live test video full of short back-and-forth phrases: every short
   genuine utterance always inherited its neighbor's language, so a short
   English word next to Tamil speech got transcribed in Tamil script instead
   of recognized as English — the model doesn't switch scripts on its own;
   a language-specific fine-tune only ever outputs the one script it knows.
4. **Transcribe each chunk** with the same `stt::transcribe_and_align` the
   default pipeline uses (Parakeet for English, the Tamil checkpoint for
   Tamil) — no new transcription logic, just called once per chunk instead
   of once for the whole file, on a slightly padded audio slice (±0.15s,
   into already-detected silence) so words right at a chunk boundary
   aren't clipped mid-word.
5. **Stitch** every chunk's words back into one timeline (shifting
   timestamps from "relative to that chunk's slice" to "relative to the
   full video") — the result is the same `PipelineResult` shape the
   single-language pipeline returns, so TranscriptPanel, jump cuts, and
   Burn & Export all work completely unchanged.

A genuinely single-language video resolves to exactly one chunk after step
3, so `transcribe_with_language_detection` skips straight to one whole-file
`transcribe_and_align` call — steps 4-5's per-chunk slicing/stitching only
actually run for a video that turns out to need them.

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

**Superseded by `npm run fetch-resources` (section 3.1) and the "Build &
distribute" section (6.1)**: run `npm run fetch-resources` before this
build to populate `resources/bin/`, `resources/llama/`, and
`resources/tts-models/en/` in one shot instead of the manual per-file
drop described above — `tauri build` then bundles whatever it finds there
automatically, no separate steps needed.

**Bundled Python environments (replacing the conda requirement).** The
`stt`/`tts`/`media-ai`/`voice-clone` conda envs are being replaced by
**relocatable, bundled Python environments** — one self-contained
[python-build-standalone](https://github.com/astral-sh/python-build-standalone)
interpreter plus that feature's pinned wheels, per env, shipped as an app
resource. `src-tauri/src/python_env.rs` resolves them at runtime *ahead
of* any system conda, so:

- **`resources/python/<env>/` present** → used directly (no conda).
- **absent** → falls back to the existing system-conda discovery,
  unchanged, so a dev box that hasn't built the packs keeps working
  exactly as sections 2 / 2.4 / 2.6 / 2.7 describe.

Build them with `node scripts/build-python-runtime.mjs --pack=core`
(`stt` only, ~300 MB, goes in the MSI so transcription works out of the
box) and `--pack=extras` (`tts` + `media-ai` + `voice-clone`, ~2.5 GB,
shipped as an optional "voice & effects" pack fetched on first use via
`model_fetch.rs`). `--only-binary=:all:` is enforced — every dependency
must resolve to a prebuilt wheel; if one doesn't, bump its pin rather than
reaching back for conda. Full walkthrough and the installer wiring are in
[`docs/RUNTIME-PACKS.md`](docs/RUNTIME-PACKS.md); the companion
[`docs/MINIMAL-FFMPEG.md`](docs/MINIMAL-FFMPEG.md) covers trimming the
484 MB of full-build ffmpeg/ffprobe down to ~80 MB (the one catch:
`librubberband`, which stock prebuilt ffmpeg lacks and `tts.rs`'s emotion
pitch-shift needs).

Model *files* were already a solved problem — see 6.1's "In-app model
download": the Tamil transcription and voice-cloning checkpoints fetch
themselves on demand. This closes the remaining gap, the Python
packages/environments themselves.

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

## 9.1. Background tray app & scheduled Instagram posting

The app can now stay running in the system tray after the window closes
(instead of quitting), launch at login, and — once fully wired up — post
finished videos to Instagram at scheduled times with a native notification
confirming each post. This is a multi-phase feature; here's where it
actually stands and how to set up what exists so far.

### What's done vs. in progress

- **Done**: tray icon + menu (Open/Quit), "Launch at login" toggle
  (Settings menu), and the background lifecycle itself — closing the
  window destroys the WebView (not just hides it, to keep idle memory
  low) and the process keeps running until Quit. See `tray.rs`.
- **Done**: connecting an Instagram account via Meta's official Graph API
  (`instagram.rs`, reachable from **Settings → Instagram**) — this
  section documents that setup.
- **Done**: the media-hosting bridge, the actual publish flow, and a
  persisted scheduler with daily/weekly recurrence — see "Posting and
  scheduling" below.

### Why this uses Meta's official Graph API, not something unofficial

Only the sanctioned `instagram_business_content_publish` permission path
is used — the same one Buffer/Later/Hootsuite use. This **only works for
an Instagram Business or Creator account linked to a Facebook Page** — a
personal account cannot be automated this way, full stop, and this
project doesn't attempt to route around that restriction. Each user
registers their **own** Meta Developer App (App ID + Secret) rather than
this project shipping a shared one — same reasoning as bringing your own
Firebase project for auth (section 4).

### Setup: Instagram account, Facebook Page, and Meta App

**1. Instagram account**
- Instagram app → your profile → **Edit Profile → Switch to Professional
  Account** → choose **Creator** or **Business** (either works).

**2. Facebook Page**
- Go to `https://www.facebook.com/pages/creation/` → name it, pick a
  category, **Create Page**. It doesn't need followers or content.
- Link it to your Instagram account via **Accounts Center**: Facebook →
  your profile picture → **Settings & Privacy → Settings → Accounts
  Center → Add accounts → Instagram**, sign into the same Instagram
  account. Confirm it under the Page's **Settings → Linked accounts**.

**3. Meta Developer App**
- Go to `https://developers.facebook.com/apps/` → **Create App** → type
  **Business** → name it, add your email, create.
- On the dashboard, **Add use cases** → filter to **All** or **Content
  management** → select **"Manage messaging & content on Instagram"**
  (not Facebook Login, not the Marketing API, not Fundraisers). Confirm.
- When asked which business portfolio to connect: pick your existing one
  rather than skipping it — no verification is required at this stage,
  and it simplifies discovering the linked Instagram account later.
- **Add the specific permissions — easy to miss, and required.** Adding
  the use case above does *not* automatically grant the underlying
  permissions; the OAuth dialog rejects them with "Invalid Scopes" until
  each is explicitly added. Open the use case → **Permissions and
  features** → click **+ Add** on exactly these five:
  - `pages_show_list`
  - `instagram_basic`
  - `instagram_content_publish`
  - `business_management`
  - `pages_read_engagement`

  Leave the `instagram_business_*` versions of these alone — those are
  scope names for a different, newer login product ("Instagram API with
  Instagram Login") this app doesn't use; requesting them against the
  classic `facebook.com/dialog/oauth` endpoint this app actually calls
  fails the same "Invalid Scopes" way, confirmed directly against a real
  app while building this.

  **Why the last two matter — Business Portfolio Pages:** if your
  Facebook Page lives inside a **Business Portfolio** (Meta Business
  Suite) rather than being a plain personally-created Page, `/me/accounts`
  (the endpoint used to list which Pages you manage) returns an empty
  list for that Page even with full Page access and the first three
  permissions granted — confirmed live against a real account. `pages_show_list`
  being silently dropped isn't the cause in that case; the Page just isn't
  reachable through that endpoint at all for a Business-Portfolio-owned
  Page. `instagram.rs` falls back to `/me/businesses` →
  `/{business_id}/owned_pages` to reach it instead, which is what
  `business_management` and `pages_read_engagement` are for. If your Page
  is a plain personal Page (not inside a Business Portfolio), the first
  three permissions alone are enough and `/me/accounts` finds it directly
  — but requesting all five up front costs nothing and avoids hitting this
  exact dead end later if you ever move the Page into a Business Portfolio.
- **App settings → Basic**: note the **App ID** and **App Secret** (click
  "Show", re-enter your Facebook password). These go into the app itself
  (see below), never into `.env` or any file that could end up in the git
  repo or a distributed build — see the note under "Storing your App
  ID/Secret" below for why that distinction matters.
- **App settings → Advanced → App authentication**: toggle **"Native or
  desktop app?" ON** (required for Meta to accept a `localhost` redirect
  URI at all) and set **Authorize callback URL** to exactly:
  ```
  https://localhost:47829/instagram/callback
  ```
  **Must be `https`, not `http`** — confirmed directly that Meta rejects
  a plain `http://localhost` redirect outright, a real, longstanding
  policy requiring OAuth redirects to be HTTPS even for localhost, not
  something specific to this app. `instagram.rs`'s local listener
  presents a self-signed certificate generated fresh per sign-in attempt
  to satisfy this — nothing needs it to be *trusted* (the connection
  never leaves your machine), just present, so your browser shows a
  one-time "connection isn't private" interstitial on that final redirect
  hop that you click through once per sign-in. Deliberately not "fixed"
  by installing a locally-trusted root CA instead — that would be a
  meaningfully more invasive, worse-security-posture change (any site
  could then be silently vouched for by that CA, and unexpected
  root-certificate installation is itself a common antivirus/EDR red
  flag) for the sake of removing one occasional click.

  Leave **"App secret embedded in client"** OFF — that setting is for
  apps that ship the secret inside something publicly distributed
  (a mobile APK, a JS bundle) and restricts it to limited "client token"
  operations as damage control for that exposure. This app never ships
  the secret anywhere; it stays in your own per-machine app-data folder,
  entered once through the app's own UI. Turning it on would break the
  token-exchange calls `instagram.rs` needs the full secret for.
- **App roles → Roles → Instagram Testers → Add Instagram Testers** →
  enter your Instagram username → send the invite. This is what lets the
  app publish immediately, without Meta's 2-4 week App Review — it only
  works for accounts explicitly added this way while the app stays in
  Development Mode.
- Accept the invite **from Instagram's side**: Instagram → profile →
  **Settings → Apps and Websites → Tester Invites → Accept**. Back on the
  Meta dashboard, the tester's status should flip from "Pending" to
  "Active".

### Connecting the account in the app

**Settings → Instagram** (top menu bar — renamed from the old "Developer"
menu, since Launch-at-login and Connect Instagram are both settings a
normal user wants, not developer-only checks; those moved behind a
"Troubleshoot" toggle in the same panel). The panel has its own
condensed, click-by-click version of everything above built in — useful
if you land here without having read this section first:
1. Enter your **Meta App ID** and **App Secret**, click **Save** — this
   calls `save_instagram_app_config`, which writes them to
   `~/.reels-caption-app/instagram-app-config.json` (or the OS
   equivalent), not anywhere touched by `npm run build` or git.
2. Click **Connect Instagram** — opens your system browser to Meta's
   consent screen (your existing Facebook session/2FA/password manager
   all just work normally, since it's a real browser, not an embedded
   webview). `instagram.rs` runs a short-lived local listener on port
   `47829` to catch the redirect, exchanges the code for a token, upgrades
   it to a ~60-day long-lived token, and discovers the linked Instagram
   Business Account through the connected Page — trying `/me/accounts`
   first, then falling back to `/me/businesses` → `/{business_id}/owned_pages`
   for Pages that live inside a Business Portfolio (see the permissions
   note above).
3. Once connected, the panel shows `Connected as @yourusername`.

### Posting and scheduling

Once an account is connected, a **Schedule to Instagram** button sits next
to **Burn & Export** — it's disabled until you've burned a video in the
current session (scheduling posts the finished captioned file, not the raw
source). Clicking it (`ScheduleToInstagramButton.jsx`) opens a small panel:

- **Post now** — publishes immediately.
- **Schedule once** — pick a date/time; fires exactly once.
- **Add to daily/weekly recurring slot** — there's at most one daily and
  one weekly schedule; each has its own FIFO queue, and this appends the
  current video (with its own caption) to whichever slot's queue. The
  first time you add to a slot, the date/time you pick sets that slot's
  time-of-day (and, for weekly, day of week) going forward.
- An **Upcoming** list shows every schedule's status, next run time, and
  queue length, with a Remove button.

**How publishing actually works** (`instagram.rs`'s `publish_reel` +
`media_host.rs`): Instagram's Graph API fetches video from a public URL
rather than accepting a direct upload, and this app has no permanent cloud
storage, so each publish:
1. Serves the video file from a tiny local HTTP server (`media_host.rs`,
   via `tiny_http` — already a dependency for the OAuth listener above).
2. Fronts it with a temporary `cloudflared` **quick tunnel** (no Cloudflare
   account needed) so Instagram can reach it over a real public URL.
   `cloudflared` itself is fetched on-demand from its own GitHub Releases
   the first time it's needed (same idea as `model_fetch.rs`'s optional
   model downloads) if it isn't already on your system PATH.
3. Creates a media container (`POST /{ig-user-id}/media`), polls it until
   Instagram reports `FINISHED` processing, then publishes it (`POST
   /{ig-user-id}/media_publish`).
4. Tears down both the tunnel and the local server immediately after,
   success or failure — there's no standing cost between posts.

**How scheduling actually works** (`scheduler.rs`): a single background
task, spawned once in `lib.rs`'s `setup()`, ticks every 60 seconds on
Tauri's own tokio runtime — no window needed, so a schedule still fires
while the app sits tray-only with the window closed. Each tick finds due
schedules, pops the next clip off that schedule's queue, calls
`publish_reel`, and fires a native OS notification confirming success or
failure (`tauri-plugin-notification`). A recurring schedule whose queue
runs dry is marked "paused (queue empty)" rather than silently doing
nothing, and un-pauses itself the next time a clip is added to it.
Recurrence is deliberately plain duration math (add exactly 24h/7×24h to
the stored next-run timestamp) rather than calendar-aware scheduling — it
drifts by daylight-saving's ~1 hour twice a year, an accepted tradeoff for
staying dependency-free.

**Token expiry**: the connected account's long-lived token lasts ~60 days,
but this refreshes itself automatically — every scheduler tick (60s) calls
`instagram::refresh_instagram_token_if_needed`, which is a no-op until the
token is within 5 days of expiring, then extends it (and re-derives a
fresh Page access token) via Meta's `fb_exchange_token` grant, the same
call used for the initial short-lived → long-lived exchange. This only
works as long as the app runs (even just tray-resident) at least once
every ~55 days — Meta's API can extend a token that still has time left,
but can't revive one that's already fully expired. If that does happen (or
if the account was connected before this refresh logic existed, which
persisted no raw user token to refresh from), a scheduled post fails with
a clear "reconnect in Settings" error (surfaced via the schedule's
`last_result` and a failure notification) rather than a silent no-op, and
reconnecting (Settings → Instagram → Connect Instagram) is the manual
fallback.

**Storing your App ID/Secret — why not `.env`:** Firebase's `.env` (section
4) works because those values are meant to be public-safe, baked into the
built JS bundle at compile time and protected by Firebase's own security
rules instead of secrecy. A Meta App Secret is a genuine secret — putting
it in `.env` would compile it into the distributed app bundle, exactly the
"secret embedded in a public client" exposure the Meta toggle above exists
to warn about. `save_instagram_app_config` instead writes it at *runtime*,
from a value you type into the app itself, straight to your own machine's
app-data folder — never built, never committed, never shipped.

---

## 9.2. Over-the-air updates

The app checks for a newer signed release automatically on launch
(`UpdateBanner.jsx`, silent unless one's actually available) and on demand
(Settings → Updates). Built on Tauri's own `tauri-plugin-updater` — no
custom update server, just a small static `latest.json` manifest and the
signed installers themselves, both hosted as GitHub Release assets (the
same free hosting this project already uses for the MSI/model downloads).
Firebase was considered and rejected for this specifically: its free-tier
Hosting bandwidth (360MB/day) is a real constraint for installer-sized
files, where GitHub Releases has no comparable limit for a public repo —
and it would mean standing up a second piece of infrastructure for
something GitHub already does for free.

### How it works

Every update is cryptographically signed and verified before it's ever
installed — this isn't optional, the updater refuses anything that doesn't
match. Two keys, generated once via `npx tauri signer generate`:
- **Public key** — safe to distribute, baked into `tauri.conf.json`'s
  `plugins.updater.pubkey` at build time, so it ships inside every install.
- **Private key** — the opposite. Never appears in the app, the repo, or
  any build artifact; it only ever lives on whatever machine actually cuts
  a release. If it ever leaked into a shipped binary, anyone could sign a
  fake "update" this app would trust — the entire point of this scheme is
  to prevent that.

There's no expiry on this keypair (unlike a TLS certificate) — it works
indefinitely unless you deliberately rotate it. What actually matters is
**backup**, not expiry: lose the private key (not leak it — just lose it,
e.g. a dead hard drive with no copy) and every already-installed copy has
the *old* public key baked in with no way to ever verify a future update
again, short of getting everyone onto a fresh manual reinstall with a new
key. Store it somewhere durable and secret (a password manager, or a CI
secret once releases move to CI) — it currently lives at
`~/.tauri/reels-caption-app.key` on the machine that generated it, **not
committed to git**, generated without a password for simplicity (minisign
supports one; add `--password` to `tauri signer generate` for extra
protection at the cost of needing to supply it at every future signing).

### Cutting a signed release: `.github/workflows/release.yml` (automatic)

Pushing a version tag is the entire release process — no manual per-OS
`tauri build`, no hand-assembling `latest.json`:
```bash
# after bumping `version` in src-tauri/tauri.conf.json and package.json, committed:
git tag v0.2.0
git push origin v0.2.0
```
That triggers a 3-OS matrix build (`windows-latest`, `macos-latest`,
`ubuntu-22.04`) via [`tauri-apps/tauri-action`](https://github.com/tauri-apps/tauri-action)
— the same action the "CI matrix" note in section 6.1 pointed at, now
actually wired up. Each OS builds and signs its own installer, and the
action creates the GitHub Release for that tag, uploads every installer,
and generates + uploads `latest.json` itself, matching the schema
`tauri.conf.json`'s `plugins.updater.endpoints` expects.

**One-time setup this can't do for you** — by design, since the whole
point of the signing scheme is keeping the private key out of anything
automatable/committable. Add these under the repo's *Settings → Secrets
and variables → Actions*:
- `TAURI_SIGNING_PRIVATE_KEY` — the contents of `~/.tauri/reels-caption-app.key`.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — only needed if that key was
  generated with a password (it wasn't, by default, per the note above).

**Real findings from this workflow's actual first run** (tagging v0.2.0):
all 3 platforms failed `tauri build` immediately, for two real, since-fixed
gaps — not something to have assumed away from just reading Tauri's docs:
- **Ubuntu**: `alsa-sys`'s build script failed outright — this app's
  live-dictation/mic-recording work (added after this workflow was first
  written) pulls in `cpal`, which links against ALSA on Linux and needs
  its real headers, not just its runtime library. Fixed by adding
  `libasound2-dev` to the existing apt-get install line (alongside
  webkit2gtk/appindicator/librsvg/patchelf).
- **All 3 platforms**: `tauri build` hard-failed with `resource path
  "resources\tts-models" doesn't exist` — `tauri.conf.json`'s declared
  bundle resources (`resources/bin`, `resources/llama`,
  `resources/tts-models`) are real files this repo deliberately never
  commits (section 3's whole `dev-resources.tar.gz` mechanism exists
  *because* of that), so a fresh CI checkout starts with none of them.
  Fixed by adding an `npm run fetch-resources` step (same script a local
  dev setup already runs) right after `npm ci`, before the build.

**Two further real findings from the next attempts, once the above two were
fixed** — the build/sign/bundle steps themselves succeeded on all 3
platforms after these, so this was genuinely the last of it:
- **`TAURI_SIGNING_PRIVATE_KEY` had never actually been added as a repo
  secret at all** (confirmed directly via the Actions secrets API — zero
  secrets existed), despite the setup note above always having said to.
  Once added, a *second* mistake happened correcting a red herring:
  `~/.tauri/reels-caption-app.key` looked "double base64-encoded" at a
  glance and was (wrongly) "fixed" by decoding it once — that broke it
  further, with a different, more specific error (`failed to decode
  base64 key: Invalid symbol 32, offset 9` — offset 9 is exactly the
  space after "untrusted" in the comment line). The file's original,
  single-line, base64-wrapped-whole-file form was correct all along —
  that *is* Tauri's real documented convention for delivering a
  multi-line key through a single-line CI secret cleanly. Restored from
  a backup taken before the wrong "fix," and re-set as the secret.
- **The corrected private key's real public half didn't match
  `tauri.conf.json`'s baked-in `pubkey`** — two genuinely different
  keypairs (confirmed by decoding both and comparing their minisign key
  IDs directly: `2784C5DB6D2E737F` vs. the configured `6B75EB61F3CF92F1`),
  not a formatting issue this time. The original keypair matching that
  configured pubkey isn't recoverable (no backup exists anywhere) — since
  v0.1.0 was only ever "a first test release" with no real installs
  auto-updating from it, decided to rotate rather than chase a lost key:
  `tauri.conf.json`'s `pubkey` now matches the private key that actually
  exists and is the one in the repo secret. Any future key loss/rotation
  needs both sides changed together like this — see the backup note above
  for why that's worth actually doing this time.
- **The default `GITHUB_TOKEN` this workflow gets is read-only** (a repo-
  level setting, invisible from this file) — `tauri build`/sign/bundle all
  succeeded, then release creation itself failed with `Resource not
  accessible by integration`. Fixed with an explicit `permissions:
  contents: write` on the job — the minimum tauri-action actually needs
  to create the release and upload assets, rather than changing the
  repo-wide default.

**What this workflow does NOT cover**: the large asset archives
(`dev-resources.tar.gz`, `optional-models.tar.gz`) are a separate,
still-manual `gh release upload <tag> ... --clobber` step (see "Publishing
a new release" above) — this workflow only builds/signs/publishes the app
itself. Upload those to the same tag's release after the workflow
finishes, same as before.

---

## 9.3. Persistent media library (local storage, project list)

Every imported video used to just be referenced at whatever OS path the
file dialog returned — never copied anywhere — and a burned/exported
output went wherever a save dialog pointed. Nothing survived switching to
a different video: no history, no way to return to a past project, no
guaranteed place either file actually lived. `library.rs` fixes that:
importing a video copies it into `app_data_dir()/media/<project_id>/original.<ext>`,
Burn & Export always writes into that same project's own
`media/<project_id>/processed/` subfolder, and a **local SQLite database**
(`library.db`, `rusqlite` with the `bundled` feature) tracks every project
— title, description, hashtags, filename, timestamps, and everything else
needed to fully resume editing.

**Local SQLite, not the flat-JSON-file pattern used elsewhere in this app**
(`scheduler.rs`, `instagram.rs`) — deliberately, since a later phase adds
local semantic (vector/cosine) search over this same data, something a
flat JSON file has no path to at all. Verified directly before writing any
code around it: `rusqlite`'s `bundled` feature (SQLite's own C source
compiled straight into this binary) builds cleanly under this project's
MinGW/GNU toolchain — the same toolchain that made sherpa-onnx's
MSVC-only prebuilt library a real, confirmed build blocker earlier in this
project's history (see the live-dictation section above), so this got
checked with a real `cargo build` up front rather than assumed to just
work.

**The `projects` table stays deliberately thin.** Only what the project
*list* itself needs (id, paths, title/description/hashtags, timestamps)
gets a real column; everything else a full editing session needs to
restore (transcript words, caption style, prosody, speakers,
voiceover/music state, detected language, generated content ideas) is one
opaque `state` JSON text column — Rust never needs to understand that
shape, only the frontend does. This is deliberate, not laziness:
`CaptionStyle` (`captions.rs`) is `Deserialize`-only, no `Serialize` — an
opaque blob sidesteps needing that (and three other structs) to grow a
second Rust-side mirror that has to stay in lockstep with the JS shape
forever.

**Burn & Export dropped its "Save As" dialog.** It now always writes into
the project's own `processed/` folder via a new `processed_output_path`
command — `burn_captions` itself needed zero changes; the frontend just
calls this in place of the old `save()` dialog and passes the result
through as the same `output_path` argument it always took. A "Reveal in
folder" link (wrapping `tauri_plugin_opener::reveal_item_in_dir`, already a
dependency) appears next to the burn status, since there's no dialog
anymore to show *where* the file went.

**Loading a project from the sidebar list never re-runs transcription.**
`App.jsx`'s `applyProjectState` is the one function both "just imported a
brand-new video" and "clicked an existing project" go through — the
difference is only whether `runPipelineFor` gets called afterward. An
autosave effect (plain debounced `setTimeout`, ~1s) keeps the currently-open
project's row up to date as you edit, with no explicit "Save" action
anywhere, matching this app's existing "no separate save step" philosophy.

**The sidebar list sorts by `created_at`, never `updated_at`** — sorting by
last-edited would make the project you're actively working on jump to the
top of its own list on every autosave tick, which is disorienting.
"Latest first" means newest *import*.

**A real, pre-existing bug this work surfaced, not caused**: `cargo test`
for this whole crate crashes with `STATUS_ENTRYPOINT_NOT_FOUND` before
running a single test — confirmed directly to predate this feature by
stashing every change from this whole session (including before
`library.rs`/`rusqlite` existed) and re-running it on that clean baseline,
where it crashed identically. This means `captions.rs`'s own 58-test suite
currently can't run via `cargo test` in this environment either, for
reasons unrelated to anything built here. `library.rs`'s own SQL logic
(schema, insert, list-ordering, update, delete, JSON round-tripping
including real Tamil UTF-8 text) was instead verified against a real
`rusqlite` connection via a standalone `cargo run` binary outside this
crate — sidesteps the broken test harness rather than being blocked by it.
Fixing the harness itself is out of scope here; `#[cfg(test)]` tests were
still added to `library.rs` (same convention as `captions.rs`) for
whenever it's fixed.

**Title/description/hashtags now auto-fill in the background**, requested
directly, rather than only on a manual "Generate" click — `runPipelineFor`
fires `generateContentIdeas` the moment a fresh transcript comes back
non-empty (a silent/no-audio video generates nothing, since there's
nothing to summarize), with a "✨ Generating…" hint in the sidebar while it
runs. The existing "only fill in while the title still reads as the
placeholder" guard is what keeps this from ever overwriting a title
someone's already started typing, even if generation is still running
when they do.

**The AI-generated description used to come back as most of the
transcript, lightly reworded — not a summary.** Confirmed directly against
the real local model, not just suspected: a genuine 138-word transcript (in
the actual unpunctuated shape real ASR output has, not a hand-punctuated
paragraph) produced a 128-word "description," essentially the whole thing.
Fixed two ways, verified together — neither alone was enough: (1)
stronger, more explicit wording telling the model never to copy or
paraphrase the transcript, just summarize the general topic — this alone
still let real generations run long; (2) a generous `maxLength` on the
JSON schema as a backstop — but verified directly that relying on *this*
alone just cuts generations off mid-sentence once they hit the cap (e.g.
"...even sending alerts when"), a real regression of its own. The actual
fix is `trim_to_sentence` (`content_ideas.rs`): applied after generation,
it trims to the last *complete* sentence within a tighter target length,
falling back to a word-boundary cut with a trailing "…" only when there's
no sentence-ending punctuation at all within budget (real transcripts
often have none). Verified end-to-end on two different real transcripts
(a DIY project, a memorial tribute): descriptions landed at 24-27 words,
complete and on-topic, regardless of the source transcript being anywhere
from 58 to 138 words.

**Burn & Export is now labeled "Save"** (`BurnExportButton.jsx`) — asked
for directly, since it's genuinely the app's save/checkpoint action: it
writes the current transcript/style/etc. as a burned video *and* now also
saves the project's full state immediately afterward (`App.jsx`'s
`burnCaptions`, via a new shared `currentProjectPayload` helper), not just
relying on the debounced autosave to eventually catch up — so reopening a
project later reliably resumes from at least that checkpoint even if the
app closed within the debounce's 1-second window. The underlying
command/prop names (`burnCaptions`, `burning`, ...) stay as they are; only
the button's label and icon changed, since they describe the actual
mechanism (burning captions into a new file), not what the button is
called.

**A way to view/play the untouched original video for reference**, also
requested directly — a "▶ View original video" toggle in the sidebar plays
`originalVideoPath` inline (a new piece of state, set once per project and
never mutated afterward). This matters because the *main* preview's own
video can silently stop being the original: a jump-cut re-points
`videoPath` at a genuinely different (silence-removed) file, and its audio
track mutes once a voiceover is active — neither of those touch
`originalVideoPath`, so the real original is always one click away
regardless of what the main preview currently shows.

**Not yet built** (see the plan this was scoped from): local semantic/
vector search over the library (via the `sqlite-vec` SQLite extension +
locally-generated embeddings — no cloud embedding API), and Cloudflare R2
overflow for older projects' video files once the library grows past
roughly the newest 50 (chosen over AWS S3 specifically for R2's permanent
free tier with zero egress fees — real cost research, not assumed, since
"stream/play" from cloud means egress is the cost that actually matters).

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
- ~~Run in the system tray, launch at login, and connect an Instagram account for future scheduled posting.~~ — new `tray.rs` (tray icon, Launch-at-login toggle, close-to-tray window lifecycle) and `instagram.rs` (Meta Graph API OAuth connect, Settings → Instagram), section 9.1. Several real bugs found via direct interactive testing: Tauri's default behavior exits the whole app once the last window is gone, even one this app destroyed itself, requiring an app-level `RunEvent::ExitRequested` handler; that fix then swallowed real Quit clicks too, since `app.exit()` turned out to route through that same preventable event rather than bypassing it as its docs suggest — fixed with a `tray::QUITTING` flag distinguishing the two. An abandoned OAuth attempt's local listener thread could permanently hold its port, fixed with a 5-minute `recv_timeout` deadline. And `/me/accounts` returns zero Pages for a Page that lives inside a Business Portfolio even with correct permissions — fixed with a `/me/businesses` → `/{business_id}/owned_pages` fallback, which needed two more scopes (`business_management`, `pages_read_engagement`) beyond the original three. The scheduler and actual publish/media-hosting flow are not built yet.
- ~~Let an installed app fetch the models it's missing, instead of a manual conda/curl dance.~~ — new `model_fetch.rs` + `shell/OptionalModelsBanner.jsx`: a one-click in-app download of a dedicated, smaller release asset (`optional-models.tar.gz`, Tamil transcription + voice cloning checkpoints only) into the same per-machine cache dir the app already falls back to. Deliberately not done inside the MSI installer itself — considered and rejected, since MSI's transactional install model handles long network operations poorly — section 6.1's "In-app model download."
- ~~Sidecar-bundle `ffmpeg` itself so users don't need it on PATH.~~ — achieved via `bin_paths.rs`'s resources-based bundling (env var override → bundled resource → PATH fallback) rather than Tauri's dedicated [sidecar API](https://v2.tauri.app/develop/sidecar/) specifically — the practical goal (no PATH dependency in a built installer) is met either way; `npm run fetch-resources`/the installer's `bundle.resources` (`tauri.conf.json`) is what actually gets `ffmpeg.exe`/`ffprobe.exe` into every build, section 3.1.
- ~~Handle a video where different speakers use different supported languages (e.g. one in Tamil, one in English), transcribing each in its own language instead of picking one language for the whole file.~~ — new `mixed_language.rs` + `ffmpeg::detect_speech_segments` (language-agnostic silence-based segmentation) + `stt::detect_spoken_languages_batch` (one model load, many segments) + a same-language-run merge step to protect transcription quality (avoids feeding the STT model lots of short, low-context clips), replacing `pipeline.rs`'s old single whole-file `detect_spoken_language` call entirely rather than sitting behind an opt-in toggle — a single-language video still collapses to one fast whole-file transcription call, so there was no real cost to always detecting this way. Two real bugs found and fixed via real bilingual test videos (see section 7.4): a confidence-vs-duration threshold ordering bug that mistrusted short genuine utterances, and Whisper's language-ID only ever examining roughly its first ~30 seconds of whatever audio it's handed.
- ~~Actually publish a finished video to Instagram, and let it be scheduled (one-off or recurring) instead of only connecting an account.~~ — new `media_host.rs` (temporary local file server + on-demand `cloudflared` quick tunnel, since Instagram fetches video from a public URL rather than accepting a direct upload) and `scheduler.rs` (persisted daily/weekly/once schedules, a 60-second background tick that runs even with the window closed, native notifications on each attempt), plus `instagram.rs`'s `publish_reel` (container → poll → publish). New **Schedule to Instagram** button (`ScheduleToInstagramButton.jsx`) next to Burn & Export. See section 9.1's "Posting and scheduling."
- ~~Automatically refresh the connected Instagram account's token instead of requiring a manual reconnect every ~60 days.~~ — `instagram.rs`'s `refresh_instagram_token_if_needed`, checked every scheduler tick: extends the long-lived user token (and re-derives a fresh Page access token) via the same `fb_exchange_token` grant used for the initial exchange, once within 5 days of expiring. Needed persisting the raw long-lived user token (`IgAccount.user_access_token`, previously discarded right after deriving the Page token) — an account connected before this only refreshes after one manual reconnect.
- ~~Over-the-air updates, hosted economically instead of standing up a new backend.~~ — `tauri-plugin-updater` + `tauri-plugin-process`, checking a static `latest.json` manifest hosted as a GitHub Release asset (same free hosting this project already uses for the MSI/model downloads) rather than Firebase Hosting, whose free-tier bandwidth is a real constraint for installer-sized files. Every update is signed (a keypair generated via `tauri signer generate`; the public half ships in `tauri.conf.json`, the private half never leaves the release-building machine) and verified before installing. `UpdateBanner.jsx` checks silently on launch; Settings → Updates checks on demand. See section 9.2 — cutting an actual signed release is still a manual, undocumented-until-now process (no CI release pipeline exists yet), now written up there.
- ~~A push-button way to record a voice-over from the mic (for captions, optionally merged in as the video's actual audio) — plus a global hotkey for it, since the app already lives in the system tray.~~ — `mic_recording.rs` (browser `MediaRecorder` capture, converted to WAV via the already-bundled ffmpeg) feeding the existing voiceover/transcription pipeline; VoiceoverSection.jsx's new "Record from mic" mode (its own checkbox for caption-only vs. replace-the-audio); `dictation.rs` + `DictationHud.jsx` for the `Ctrl+Shift+D` global-hotkey floating HUD, reachable even with the main window closed. See section 2.8.
- ~~Make the text actually appear as you speak, not just after recording stops — as a unique, lightweight, separate workflow, capable of Tamil+English code-switching too.~~ — `streaming_stt.rs`: `cpal` for native OS mic capture, feeding a periodic (~2.5s) re-transcription through the exact same `mixed_language.rs` code-switching pipeline a whole video already uses, instead of a from-scratch streaming implementation. Both `LiveDictationPanel.jsx` (in-app) and the `Ctrl+Shift+D` global-hotkey HUD (`dictation.rs`/`DictationHud.jsx`) use this same backend now. First attempt used `sherpa-onnx` for true incremental streaming (smooth per-word text, but English-only, and hit a real MSVC-vs-GNU-MinGW linking build failure along the way, fixed by switching to its `shared`/DLL feature) — replaced once it became clear no streaming-capable model exists for Tamil, confirmed via k2-fsa's own project discussion. Real on-device testing then surfaced several more bugs, each fixed the same way (try it, watch it fail, fix the actual cause): (1) it didn't actually feel live — root cause was each ~2.5s cycle spawning a brand-new Python process and reloading every model from scratch, fixed by replacing that with `stt/live_worker.py`, a single persistent Python process (loaded once, kept alive for the whole session) served over a newline-delimited JSON stdin/stdout protocol (`LiveWorkerState`/`ensure_live_worker` in `streaming_stt.rs`) — verified directly against the real conda env outside the app (a standalone probe script feeding it real synthesized speech), not just by re-reading the code: cold model load is a real, one-time ~20-25s cost, and every request/response after that round-trips correctly; (2) the HUD's close button called the window-close API directly, which the `dictation-hud` capability didn't actually grant permission for (`core:default` doesn't include it) — it silently did nothing, and worse, since it bypassed `stop_live_dictation` entirely, it left the previous session's mic stream running forever; (3) `<React.StrictMode>` in `dictation-hud-main.jsx` double-invokes the HUD's mount effect in dev, which fired `start_live_dictation` twice in a race and made even a *first, fresh* hotkey press fail with "a session is already running" — fixed by dropping StrictMode for that one entry point, since its mount effect starts a real non-idempotent backend session (a mic-capture thread and a worker process), not the kind of effect StrictMode's double-invoke is meant to protect; (4) starting a new session while a previous one was still (or stuck) running used to simply refuse with that same "already running" error — changed to force-end whatever was there instead (signal its stop flag, wait for its capture thread to actually release the microphone, abort its transcription task) before opening a new one, since a `cpal` input stream left open by a stale session could keep a *new* one from ever capturing real audio at all, which looks exactly like "live dictation never shows any text" rather than like a session conflict; (5) the HUD could hang forever on "Finishing…" with nothing clickable, because `stop_live_dictation` used to block until the final transcription pass completed — fixed by making it signal-and-return immediately (the real result arrives later via the existing `live-dictation-final` event) and giving every HUD phase an always-visible close button that now actually works. Also added a `live-dictation-worker-status` event so the UI shows "Loading speech models (first time only)…" during that one-time cold-start cost instead of silently showing nothing, which had looked indistinguishable from "broken." See section 2.8's "Live dictation."
- ~~Fix the live preview playing both the original video's audio and a new mic-recorded voice-over at once, even with "replace its audio" checked.~~ — the burned/exported file was always correct (`captions.rs`'s `-map 1:a` genuinely drops the original track); only `VideoPreview.jsx`'s *live* preview was affected. `videoRef.current.muted = true` was already being set imperatively once a voiceover went active, but nothing kept it that way afterward — the `<video>` element still has `controls` (for scrubbing/fullscreen), and its native player chrome has its own volume/mute button that can flip `.muted` back to `false` completely outside React, playing the original audio back alongside the voiceover. Fixed with an `onVolumeChange` handler that re-asserts `muted = true` any time it fires while a voiceover is active, which covers every route back to unmuted (the native button, a dragged volume slider, an OS media key), not just the one that was actually hit.
- ~~Clean up a "Record from mic" take: cut the dead air at the start, and make the speech itself sound clearer.~~ — `mic_recording.rs`'s `save_recorded_voice` now runs one combined ffmpeg filter chain (`highpass` for rumble, `afftdn` for noise, `silenceremove` for leading silence only, single-pass `loudnorm` for clarity/level) instead of a bare format conversion. Verified directly against a real synthesized-speech clip with 2s of silence prepended, not just by reading ffmpeg's docs: the trimmed/cleaned output still transcribed word-for-word with nothing clipped off the start, and forcing `-ar 16000 -ac 1` on the *output* was necessary because `loudnorm` resamples internally (confirmed directly: an unconstrained pass on 16kHz input came out at 192kHz, which would have quietly broken the "16kHz mono WAV" contract every other audio path in this app relies on).
- ~~Fix a genuinely-Tamil mic recording coming back with an empty transcript and "Detected language: English."~~ — root-caused against the actual real recording that triggered it (kept in temp from the bug report, not reconstructed): `pipeline.rs`'s `transcribe_audio_file` (every voiceover/mic-recording path routes through this) was reusing `mixed_language.rs`'s video-oriented language detector, which chops audio on every natural pause (~0.5s+) and classifies each resulting sliver (often 1-4s) independently. On this real ~53s recording that produced wildly inconsistent per-sliver guesses across half a dozen unrelated languages, and by pure chance the only two slivers that crossed the confidence bar both said "English" — which then forward/backward-filled across the *entire* recording, silently routing genuinely Tamil audio through the English-only transcriber. Forcing the same file through the Tamil transcriber directly proved the audio itself was fine all along: a full, coherent 37-word Tamil transcript came right out. Fixed with a new `transcribe_solo_recording_with_language_detection` (`mixed_language.rs`) specifically for this single-speaker-single-take case (a voiceover recording, unlike a full video, never has a genuine within-clip language switch to catch): classify a handful of large (~20s) windows across the clip and majority-vote the supported-language guesses, instead of dozens of tiny, easily-wrong ones. Verified directly on the same real recording: three large windows never once said "English" (the two that missed Tamil said Telugu/Malayalam instead — still wrong individually, but neither is a *supported* language here, so neither could hijack the vote), and the majority-vote result came back Tamil. A second real recording then hit the case where *every* window missed Tamil entirely (both said Telugu) — surfaced correctly as "Detected spoken language 'te' isn't supported yet" rather than silently mistranscribing, but genuinely still Tamil underneath. Added one narrow, evidence-based fallback for exactly this: whisper-tiny's language-ID has no special training to tell South Dravidian languages apart (Tamil/Telugu/Malayalam/Kannada share enough acoustic structure to confuse a small general-purpose model despite having entirely different scripts) — confirmed on both real recordings, whose wrong guesses were consistently Telugu/Malayalam, never an unrelated language family. Since Tamil is the only Dravidian language this app can transcribe, seeing Telugu/Malayalam/Kannada among the guesses (with no supported-language vote at all) now resolves to Tamil instead of erroring; an unrelated wrong guess (French, Mandarin, ...) still hits the real "unsupported language" error, since forcing genuinely different phonetics through the Tamil-specific fine-tune would produce confident-looking wrong-script nonsense, not real text — this is deliberately not a general "guess the nearest supported language" policy.
- ~~AI-generated background music, matched to the speech, instead of only ducking a music file the user already had.~~ — new `music_gen.rs` + a "✨ Suggest music" button in the existing `DuckingPanel.jsx` (sets the same `musicPath` state the upload flow already drives, so the duck-level slider and mixing step needed zero changes). Meta's MusicGen-Small (`facebook/musicgen-small`, official `transformers` model) — TinyMusician was considered first but ruled out, confirmed via web search to have no public checkpoint/package/repo, just a Sept 2025 arXiv paper. Runs in the *existing* `tts` conda env (already had `torch`+`transformers`+`scipy` for MMS-TTS) rather than a new one — verified directly that its installed `transformers` (5.15.1) already supported `MusicgenForConditionalGeneration` before writing any wiring code. Real, measured (not assumed) CPU generation speed shaped the design: ~12x slower than real-time on this dev machine (a 20s clip took ~4m12s warm, no download) — reworked, after direct feedback, from a single generate-and-apply button into suggest-then-pick: 3 short (6s) preview clips generate up front and play inline, and only the one actually chosen gets re-generated at full length (looped via `ffmpeg::loop_audio_to_duration` past 15s) and applied — auditioning cheap previews before paying full cost for just the winner, rather than generating (and mostly discarding) 3 full-length beds. Getting the LLM to write 3 good, purely-instrumental, genuinely-different, culturally-aware suggestions from a 0.5B model took five verified-against-the-real-model iterations: a plain instruction made it echo its own wording back as "suggestions"; one few-shot example fixed that but let a suggestion slip in "a soft, emotive vocal, like a lead singer" (a real bug — that phrase would push MusicGen toward vocals, not an instrumental bed); naming three explicit instrumentation categories (acoustic/electronic/percussion) fixed both the vocal leakage and a diversity collapse the stronger wording alone had caused; a `Language:` field plus a Tamil few-shot example got it correctly leaning Carnatic/Kollywood-style instrumentation for Tamil content (also surfaced a real limitation: heavily code-switched Tamil+English transcripts can derail this small model into describing the transcript's *content* instead of music, e.g. describing a recipe's ingredients — no fix shipped for that, documented instead); naming *regional folk* styles too (Gana, Chennai's rhythmic street/folk genre) got it correctly picked up for an energetic working-class-themed Tamil transcript while a calmer Tamil transcript still correctly favored Carnatic — verified it reads content, not just language, before trusting it. Asked directly whether the full context actually feeds the suggestion step: yes, the LLM sees the whole transcript (MusicGen itself only ever gets the short resulting style sentence, deliberately — its text encoder is built for short captions, not transcripts) — but that surfaced a real, previously-unbounded-transcript bug shared with `content_ideas.rs`: `llama-server`'s 4096-token context is a hard limit (confirmed directly — an oversized request gets a plain HTTP 400, not silent truncation), and measuring its real tokenizer found Tamil script costs ~8.6 tokens/word versus English's ~1.0, so a *moderate* Tamil video, not just an extreme one, could hit it. Fixed with `transcript_text_for_prompt` (language-aware word budget; a longer transcript samples its beginning/middle/end rather than just its intro) — verified end-to-end against a real 1000-word simulated Tamil transcript that previously would have needed ~8,600 tokens on its own: truncated it to ~310 words, the real request measured 3,261 tokens, and generation succeeded. Also now plays live in the preview, ducked in real time (a JS reimplementation of `ducking.rs`'s own duck-curve math, verified against real sample timestamps) — previously a bed was only ever audible after actually exporting via "Add music with ducking." See section 2.6.1.

- ~~Persist every uploaded original video and every burned/exported video into real local storage, keep a searchable, paginated, newest-first project list, and reload full editing state on click.~~ — new `library.rs`: a local SQLite database (`rusqlite`, `bundled` feature — verified directly to build cleanly under this project's MinGW/GNU toolchain before writing anything around it) tracks one row per imported video, which gets copied into `app_data_dir()/media/<id>/original.<ext>` the moment it's picked (never left at the OS path the file dialog returned). Burn & Export dropped its "Save As" dialog entirely — output now always lands in that same project's `media/<id>/processed/` folder via a new `processed_output_path` command, with `burn_captions` itself completely unchanged (the frontend just calls this instead of `save()` and passes the result through as the same `output_path` argument). `Sidebar.jsx` is now the real project list (title/description/hashtags/filename per row, click-to-load, delete, client-side pagination, sorted by `created_at` — deliberately not `updated_at`, which would make the project you're actively editing jump to the top of its own list on every autosave tick). A debounced (~1s) autosave effect in `App.jsx` keeps the open project's row current with no explicit "Save" button, matching this app's existing "no separate save step" philosophy; `applyProjectState` is the one function both "just imported a video" and "clicked an existing project" go through, and only the former also triggers transcription. The project's title/description/hashtags stay separate from (but seed once from) the existing AI-generated `contentIdeas` — requested directly: rather than only filling in on a manual "Generate" click, `runPipelineFor` now fires `generateContentIdeas` in the background the moment a fresh transcript comes back non-empty (a silent/no-audio video generates nothing, since there's no transcript to work from), so a title/description/hashtags are usually already waiting by the time you look at the sidebar. The existing "only fill in while the title still reads as the placeholder" guard is what keeps this from ever clobbering a title you've already started editing, even if generation is still running in the background when you start typing. Surfaced a real, pre-existing, unrelated bug while verifying: `cargo test` for this whole crate crashes with `STATUS_ENTRYPOINT_NOT_FOUND` before running a single test — confirmed directly by stashing every change from this entire session (including before this feature existed) and reproducing the identical crash on that clean baseline, meaning `captions.rs`'s own 58-test suite has been silently unrunnable via `cargo test` in this environment regardless of this work. Verified `library.rs`'s actual SQL logic (schema, insert, newest-first ordering, update, delete, JSON round-tripping including real Tamil UTF-8 text through the opaque `state` column) a different way instead — a standalone `cargo run` binary outside this crate, sidestepping the broken harness — all 11 real checks passed; `#[cfg(test)]` tests were still added to `library.rs` for whenever that harness gets fixed. See section 9.3. **Not yet built** (from the same plan): local vector/cosine search over the library (`sqlite-vec`, fully offline, no cloud embedding cost) and Cloudflare R2 overflow for older projects once the library grows past ~50 (R2 chosen over S3 after real research — a permanent free tier with zero egress fees, which is what actually matters for "stream/play," versus S3's smaller permanent allowance and time-limited transfer credit).

Still open:
1. Live preview doesn't yet replicate every *burn animation* (karaoke fill, pop, bounce, typewriter, per-word highlight, slide, zoom, fade) — `VideoPreview.jsx` already shows real captions, live, correctly positioned and timed over the actual video frame (not a static style swatch), and cascade mode's per-word size/color pop is matched exactly, but classic mode's `animation` setting only affects the final burned output today; the live preview shows plain styled text for all of them.
2. A path to bundling the `stt`/`tts`/`media-ai`/`voice-clone` conda *environments* themselves for zero end-user setup — see section 8.1's regression note. This is now a narrower gap than it used to be: `npm run fetch-resources` (developers) and the in-app "Download now" banner (installed end users, section 6.1) already handle every large model *file*, including the two that used to require a manual per-machine setup (Tamil transcription, voice cloning). What's left is specifically the Python packages/environments (torch, transformers, faster-whisper, openvoice, mediapipe, librosa, ...) — `conda-pack` is worth revisiting now that `stt`'s own dependency surface is lighter than it used to be.
3. More Indian languages in `stt::INDIC_LANGUAGES`/`tts::TTS_INDIC_LANGUAGES` beyond Tamil — see `resources/stt-models/README.md`. Extending the auto-detection language list (section 7) is the same piece of work now that language selection is automatic rather than a dropdown.
4. The local media server (`media_host.rs`) always returns the whole video file and ignores `Range` request headers — untested against a video large/slow enough for Instagram's fetcher to actually need partial/resumable requests.
5. AI music suggestions degrade on a heavily code-switched (Tamil+English mid-sentence) transcript — confirmed directly, not hypothesized, see `MUSIC_PROMPT_SYSTEM`'s doc comment in `music_gen.rs`. Qwen2.5-0.5B-Instruct's non-English/code-switched comprehension just isn't strong enough at this size; a real fix would need a translation-to-English step before deriving suggestions, which this app doesn't have today.
6. ~~`content_ideas.rs`'s title/hashtag generation feeds the LLM the *entire* transcript with no length budget...~~ — fixed: the budgeting logic was factored out of `music_gen.rs` into a new shared `llm_budget.rs` (`transcript_word_budget`/`transcript_text_for_prompt`, same language-aware token costs and beginning/middle/end sampling), and `content_ideas.rs`'s replacement command (`suggest_content_strategy`, see section 2.3) reuses it directly instead of duplicating the constants a second time.
7. A Tamil transcript given to `suggest_content_strategy` **with no hints** can produce a fully coherent, well-formed strategy about an entirely fabricated, unrelated topic — confirmed directly across 3 separate real test runs on the same transcript, each inventing a different unrelated topic (a furniture-shop marketplace, a coffee shop, a rainbow-vegetable recipe — none present in the actual transcript, which is a generic "today we'll talk about how we finished this new project" line). This is a harder failure than `music_gen.rs`'s already-documented Tamil weakness (that one degrades *style-matching* on code-switched content; this one fabricates the *topic* outright on plain Tamil). Not fixed here — giving real hints alongside a Tamil transcript reliably anchors the topic even when transcript comprehension itself fails, so the practical mitigation is encouraging hints for Tamil content, not a prompt fix. A real fix would need the same translation-to-English step called out in item 5, which this app still doesn't have.

Everything above runs 100% locally — `llama-server.exe` is a *local* HTTP
server bound to `127.0.0.1` only, not a remote one, so this is still
no cloud calls and no per-user cloud cost.
