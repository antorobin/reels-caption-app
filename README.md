# Reels Caption App

A local-first **Tauri (Rust) + React** app that captions video: pick a
video → extract audio + transcribe with word-level timestamps (`ffmpeg` +
`whisper.cpp`) → style the captions → burn them back onto the video
(`ffmpeg` + libass). Targets macOS, Windows, Linux, iOS, and Android from
one codebase.

## What's in the box

```
reels-caption-app/
├── src/                            # React frontend
│   ├── components/
│   │   ├── CaptionStyleEditor.jsx   # font/color/position/animation controls + live preview
│   │   └── TranscriptPreview.jsx    # word-level transcript display
│   ├── main.jsx
│   ├── App.jsx                      # wires together: pick video → transcribe → style → burn
│   └── styles.css
├── src-tauri/                      # Rust backend (Tauri)
│   ├── src/
│   │   ├── lib.rs                   # Tauri command registration + greet/check_ffmpeg/check_whisper
│   │   ├── pipeline.rs              # extract audio (ffmpeg) + transcribe (whisper-cli) -> word timestamps
│   │   ├── captions.rs              # ASS subtitle generation + burn-in via ffmpeg's `ass` filter
│   │   ├── model.rs                 # resolves the whisper.cpp GGML model file to use
│   │   ├── util.rs                  # shared temp-file helper
│   │   └── main.rs                  # desktop entry point
│   ├── resources/models/            # drop a ggml-*.bin model here (see section 6)
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

## 2. Install ffmpeg and whisper.cpp locally (desktop only, for now)

**ffmpeg**
- macOS: `brew install ffmpeg`
- Windows: `winget install ffmpeg` (or download from ffmpeg.org and add to PATH)
- Linux: `sudo apt install ffmpeg`

**whisper.cpp**
```bash
git clone https://github.com/ggerganov/whisper.cpp
cd whisper.cpp
cmake -B build
cmake --build build --config Release
# download a small model to test with
sh ./models/download-ggml-model.sh base.en
```
This produces a `whisper-cli` (or `main`, depending on your whisper.cpp
version) binary. Add it to your PATH, or later bundle it as a Tauri
**sidecar binary** (Tauri's mechanism for shipping a CLI tool inside your
app bundle) — see https://v2.tauri.app/develop/sidecar/ when you get there.

The transcription pipeline (section 4 below) asks `whisper-cli` for
word-level timestamps with `-ojf -sow`. If your whisper.cpp build predates
those flags, update the `Command::new("whisper-cli")` args in
`src-tauri/src/pipeline.rs` to match your version's CLI. (Confirmed working
against whisper.cpp 1.9.1's JSON output shape as of writing.)

**Building whisper.cpp on Windows without Visual Studio:** `cmake -B build`
defaults to a Visual Studio generator, which needs the C++ Build Tools. If
you're using a plain MinGW-w64 toolchain instead (see the Rust/GNU note in
section 1), configure with Ninja and the MinGW compilers explicitly:
```bash
cmake -G Ninja -B build -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_C_COMPILER=gcc -DCMAKE_CXX_COMPILER=g++
cmake --build build
```
The resulting `whisper-cli.exe` dynamically links `libgomp-1.dll`,
`libstdc++-6.dll`, `libwinpthread-1.dll`, `libgcc_s_seh-1.dll`, and
`libdl.dll` from your MinGW install — copy those alongside `whisper-cli.exe`
(or make sure your MinGW `bin/` is on PATH) or you'll get a
`STATUS_DLL_NOT_FOUND` error when running it.

## 3. Install project dependencies

```bash
cd reels-caption-app
npm install
```

## 4. Run in development (desktop)

```bash
npm run tauri dev
```

This opens a native window with, in order:
1. **Say hello from Rust** — confirms the JS ↔ Rust bridge works.
2. **Check ffmpeg version** — confirms Rust can shell out to ffmpeg.
3. **Check whisper.cpp binary** — confirms Rust can find your whisper build.
4. **Select a video** — native file picker (`@tauri-apps/plugin-dialog`).
5. **Run transcription pipeline** — extracts 16kHz mono audio with ffmpeg,
   transcribes it with `whisper-cli`, and shows the word-level transcript.
   Needs a model at `src-tauri/resources/models/ggml-base.en-q5_0.bin` —
   see section 6.
6. **Style your captions** — font, size, color, outline, position
   (top/middle/bottom), animation preset (none/fade/pop/karaoke), and words
   per line, with a live preview swatch.
7. **Burn captions & save video** — prompts for a save location, generates
   an `.ass` subtitle file from the styled transcript, and re-encodes the
   video with ffmpeg's `ass` filter (libass) to burn the captions in.

If ffmpeg/whisper/the model aren't found, you'll see a clear error message
instead of a crash — that's intentional so you can debug setup issues
early.

## 5. Add an app icon (needed before bundling, not needed for `dev`)

This scaffold ships with no icon files. Generate a full icon set from one
square PNG (1024x1024 recommended):
```bash
npm run tauri icon path/to/your-logo.png
```
This creates `src-tauri/icons/` with all required sizes and formats. `tauri dev`
works fine without this step; `tauri build` needs it.

## 6. Bundling the whisper model

`src-tauri/tauri.conf.json`'s `bundle.resources` maps
`src-tauri/resources/models/*` into the app bundle's resource directory, so
end users don't need a separate model-download step. `check_whisper` /
`run_pipeline` resolve the model via (in order): the
`REELS_CAPTION_APP_WHISPER_MODEL` env var, the bundled resource, then
`~/.reels-caption-app/models/`. See `src-tauri/resources/models/README.md`
and `src-tauri/src/model.rs`.

This scaffold does **not** ship an actual `.bin` — even quantized it's tens
of MB and not something to vendor blind. Fetch one, quantize it, and drop
the quantized copy in yourself:

```bash
cd whisper.cpp
sh ./models/download-ggml-model.sh base.en
./build/bin/whisper-quantize models/ggml-base.en.bin models/ggml-base.en-q5_0.bin q5_0
cp models/ggml-base.en-q5_0.bin ../reels-caption-app/src-tauri/resources/models/
```

Why quantized: benchmarked on this project (55s of audio, 8 threads) at
~9.34s vs ~9.90s for the plain f16 model — a modest but real ~6% speedup,
with a byte-for-byte identical transcript (no accuracy tradeoff), and it
shrinks the bundled download from ~148MB to ~55MB. `model.rs` also still
recognizes the plain `ggml-base.en.bin` filename as a fallback if you'd
rather skip the quantize step.

If you want to trade real accuracy for more speed, `tiny.en` benchmarked
~32% faster than base.en f16 here — but visibly worse on the same test
audio (dropped punctuation, a garbled repeated phrase), so it's not the
default. Swap `BUNDLED_MODEL_FILENAMES` in `model.rs` if you want it
anyway.

## 7. Build a distributable desktop app

```bash
npm run tauri build
```
Outputs installers in `src-tauri/target/release/bundle/` — `.dmg`/`.app`
on macOS, `.msi`/`.exe` on Windows, `.deb`/`.AppImage` on Linux.

---

## 8. Adding iOS and Android

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

### ⚠️ The mobile catch: ffmpeg and whisper.cpp

`check_ffmpeg`/`check_whisper` and the transcription/burn pipeline
(`pipeline.rs`, `captions.rs`) all shell out to `ffmpeg`/`whisper-cli` as
**separate CLI processes**. iOS and Android don't allow apps to spawn
subprocesses or ship arbitrary CLI binaries — everything has to run
**in-process** via compiled libraries.

To get real video/caption processing working on mobile, you'll eventually
swap the *implementation* (not the command names, not the React UI) of
these Rust functions:

- **ffmpeg → mobile:** use [`ffmpeg-kit`](https://github.com/arthenica/ffmpeg-kit) (has iOS/Android builds you link against), called via Rust FFI.
- **whisper.cpp → mobile:** use [`whisper-rs`](https://github.com/tazz4843/whisper-rs), a Rust binding to whisper.cpp's C API — compiles into your Rust binary directly, works on all 5 platforms including mobile, no subprocess needed.

Honestly — **`whisper-rs` is worth switching to for desktop too.** Using
one FFI-based approach everywhere (instead of subprocess-on-desktop /
FFI-on-mobile) means one code path instead of two.

**This has not been done yet** — see the comment in the `[dependencies]`
section of `src-tauri/Cargo.toml`. It's deliberately deferred: `whisper-rs`
compiles whisper.cpp from C/C++ source via `cmake` in its build script, so
adding it is a real dependency-surface change, not a drop-in swap — budget
time to get the cmake/C++ toolchain building cleanly on every target
platform before you touch `pipeline.rs`.

---

## 9. Suggested next build steps

Done in this scaffold:
- ~~Add a file picker so users can select a video.~~ — `pickVideo()` in `src/App.jsx` + `@tauri-apps/plugin-dialog`.
- ~~Add a Rust command that runs the full pipeline: extract audio → transcribe → return word-level timestamps as JSON.~~ — `run_pipeline` in `src-tauri/src/pipeline.rs`.
- ~~Build the caption-styling UI in React.~~ — `src/components/CaptionStyleEditor.jsx`.
- ~~Add a Rust command that burns styled captions back onto the video.~~ — `burn_captions` in `src-tauri/src/captions.rs`, via ffmpeg's `ass` filter.
- ~~Package whisper model files as app resources.~~ — section 6 above.

Still open:
1. Swap `check_whisper`/`transcribe()`'s subprocess call for `whisper-rs` (unifies desktop + mobile — see section 8).
2. Let users edit transcript text/timestamps in the UI before burning (currently read-only in `TranscriptPreview.jsx`).
3. Sidecar-bundle `ffmpeg`/`whisper-cli` themselves (https://v2.tauri.app/develop/sidecar/) so users don't need them on PATH.
4. Real-time WYSIWYG caption preview over the actual video frame, not just the style swatch.

Everything above runs 100% offline — no server, no per-user cloud cost.
