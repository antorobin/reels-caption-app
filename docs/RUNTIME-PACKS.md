# Zero-setup runtime: bundled Python, no conda

Goal: a user downloads the MSI, installs, and every feature works — no
`conda create`, no manual anything.

## The pieces

| Piece | Where it lives | Size | In base MSI? |
|---|---|---|---|
| Minimal ffmpeg/ffprobe | `resources/bin/` | ~80 MB (from 484) | yes — see [MINIMAL-FFMPEG.md](MINIMAL-FFMPEG.md) |
| LLM (llama.cpp + Qwen gguf) | `resources/llama/` | ~470 MB | yes (or make it a first-run fetch to shrink further) |
| `stt` Python env | `resources/python/stt/` | ~300 MB | **yes** — transcription is the gateway feature |
| `tts` + `media-ai` + `voice-clone` envs | `resources/python/<env>/` | ~2.5 GB combined | **no** — optional "voice & effects" pack, fetched on first use |

`python_env.rs` already resolves `resources/python/<env>/` ahead of any
system conda. Nothing else in the Rust code changes.

## Build the envs

```bash
node scripts/build-python-runtime.mjs --pack=core     # stt  -> resources/python/stt/
node scripts/build-python-runtime.mjs --pack=extras   # tts, media-ai, voice-clone
```

The script pulls a relocatable [python-build-standalone](https://github.com/astral-sh/python-build-standalone)
interpreter per env and `pip install --only-binary=:all: -r
requirements/<env>.txt` into it. `--only-binary` is deliberate: if a
package needs a compiler, bump its pin — do not reintroduce conda.

Pin the `requirements/*.txt` files to the exact versions from a known-good
conda env first (`conda run -n stt python -m pip freeze`), so a pack
rebuild is reproducible.

### Prune before packing

pip leaves weight that nothing needs at runtime:

```bash
# from resources/python/<env>/
find . -type d -name __pycache__ -prune -exec rm -rf {} +
find . -type d -name tests -prune -exec rm -rf {} +
find . -type d -name test -prune -exec rm -rf {} +
rm -rf Lib/site-packages/pip Lib/site-packages/setuptools   # keep only what imports
```

torch trims hard — `torch/test/`, `torch/include/`, the `*.lib` import
libs, and the CUDA `.dll`s in a CPU build are all removable (~300 MB off
`tts`/`voice-clone`).

## Two installers

| | Bundles | MSI size | How |
|---|---|---|---|
| **Slim** (default) | app + fonts only | ~20 MB | `npm run build:slim` |
| **Full** (offline) | whole runtime | ~2–3 GB | populate resource dirs, then `npm run build:full` |

The slim installer ships nothing heavy; `runtime_fetch.rs` downloads the
components on first launch (`tier: "core"`) and on first use of a feature
(`tier: "on-demand"`). `src-tauri/tauri.full.conf.json` is the overlay
that re-adds everything to `bundle.resources` for the offline build.

## The component manager (`runtime_fetch.rs`)

Driven by `src-tauri/components.json` — compiled in as the baseline,
overridden at runtime by a hosted copy at `manifest_url`. Each component:

```
{ id, tier: "core"|"on-demand", version, url, sha256, size,
  unpack_to,          # relative to ~/.reels-caption-app/runtime/
  needed_for: [...] }  # feature ids, for the download-gate copy
```

| Component | unpack_to | Archive contents | Compressed |
|---|---|---|---|
| `ffmpeg` | `bin` | `ffmpeg.exe`, `ffprobe.exe`, `*.dll` | ~30 MB |
| `python-stt` | `python` | `stt/` | **~114 MB** (measured) |
| `llm` | `llama` | `llama-server.exe`, `*.dll`, `*.gguf` | ~460 MB |
| `python-voice` | `.` | `python/{tts,media-ai,voice-clone}/`, `tts-models/en/` | ~1 GB |

Commands: `list_runtime_components`, `missing_core_components` (drives the
first-run screen), `download_runtime_component(id)` (resumable via HTTP
`Range`, SHA-256-gated, unpacks with `tar -xf`). Progress arrives on the
`runtime-component-progress` event with the component id in the
`project_id` slot.

Resolution: `bin_paths.rs`, `python_env.rs`, `llm.rs`, `tts.rs` each check
`~/.reels-caption-app/runtime/<...>` ahead of their bundled-resource path,
so a downloaded component is found exactly like a bundled one — the slim
and full builds take the same code path.

## Build & publish (CI)

```bash
node scripts/build-python-runtime.mjs --pack=all     # resources/python/*
npm run fetch-resources                               # resources/bin, resources/llama, resources/tts-models
# ...replace resources/bin/{ffmpeg,ffprobe}.exe with a minimal build (docs/MINIMAL-FFMPEG.md)
node scripts/pack-components.mjs --out dist/components --version <YYYY.N>
```
`pack-components.mjs` writes the `.tar.gz` archives, computes SHA-256s,
and emits a filled `components.json`. `release.yml` uploads all of them as
assets on the same Release the updater already reads. Until that runs, a
slim build's `components.json` has empty checksums and the downloader
refuses every component (override with
`REELS_CAPTION_APP_ALLOW_UNVERIFIED_COMPONENTS=1` for local testing).
