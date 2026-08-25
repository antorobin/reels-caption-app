# Bundled ffmpeg binaries

Drop `ffmpeg.exe` and `ffprobe.exe` here and they will:

- be picked up automatically during `npm run tauri dev` (falls back to PATH
  if this folder is empty), and
- get bundled into the installer by `npm run tauri build` (see `resources`
  in `src-tauri/tauri.conf.json`) so end users don't need to install ffmpeg
  separately.

## ffmpeg.exe / ffprobe.exe

Grab a full/static Windows build (e.g. the "full_build" from
[gyan.dev](https://www.gyan.dev/ffmpeg/builds/)) and copy `bin/ffmpeg.exe`
and `bin/ffprobe.exe` here. The official static builds have no external DLL
dependencies (confirmed directly — no MinGW runtime DLLs needed alongside
them, unlike whisper.cpp used to require).

This file itself is just a placeholder so the `resources/bin/` directory
exists in source control — the actual `.exe` files are intentionally not
committed (ffmpeg alone is 200MB+); see the root `.gitignore`.
