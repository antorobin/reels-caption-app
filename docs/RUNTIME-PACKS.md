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

## Wire into the installer

Add to `src-tauri/tauri.conf.json` → `bundle.resources` (only after the
dir exists, or `tauri build` errors on the missing glob):

```json
"resources": {
  "resources/bin/*": "bin/",
  "resources/llama/*": "llama/",
  "resources/tts-models": "tts-models",
  "resources/fonts/*": "fonts/",
  "resources/fonts-license/*": "fonts-license/",
  "resources/python/stt/**/*": "python/stt/"
}
```

Do **not** add `resources/python/tts/**` etc. here — those ship in the
optional pack.

## The optional "voice & effects" pack

`tar czf voice-effects-pack.tar.gz -C src-tauri/resources/python tts media-ai voice-clone`,
attach it to the GitHub Release as `voice-effects-pack.tar.gz`, and extend
`model_fetch.rs`:

- add a `voice_effects` field to `OptionalModelStatus`, checked by the
  presence of `~/.reels-caption-app/python/tts/python.exe`
- add its URL alongside `RELEASE_ASSET_URL`
- `download_optional_models` (or a sibling command) fetches + untars it
  into `~/.reels-caption-app/python/`

`python_env.rs`'s `resolve_bundled_base` already checks the packaged
resource dir; add `~/.reels-caption-app/python/` as a third candidate
there so a downloaded pack is found the same way a bundled one is.

## CI

`release.yml` needs a step (Windows runner) that runs
`build-python-runtime.mjs --pack=core`, prunes, and lets `tauri-action`
bundle it; plus a job that builds `--pack=extras`, tars it, and uploads it
as a release asset. Without CI this is an unrepeatable manual chore.
