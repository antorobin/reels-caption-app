# Bundled whisper.cpp model

Drop a whisper.cpp GGML model file here named `ggml-base.en.bin` and it will:

- be picked up automatically by `check_whisper`/`run_pipeline` during `npm run tauri dev`, and
- get bundled into the installer by `npm run tauri build` (see `resources` in
  `src-tauri/tauri.conf.json`) so end users don't need a separate model download step.

Get one via whisper.cpp's downloader:

```bash
cd whisper.cpp
sh ./models/download-ggml-model.sh base.en
cp models/ggml-base.en.bin ../reels-caption-app/src-tauri/resources/models/
```

Want a different model (e.g. `small.en` for better accuracy, or a smaller
`tiny.en` for speed)? Either rename it to `ggml-base.en.bin`, or change
`BUNDLED_MODEL_FILENAME` in `src-tauri/src/model.rs`.

This file itself is just a placeholder so the `resources/models/` directory
exists in source control — the actual `.bin` is intentionally not committed
(it's 100MB+); see the root `.gitignore`.
