# Bundled TTS voices

This directory holds the Piper voice(s) for English text-to-speech under
`en/` (`*.onnx` + `*.onnx.json`). Not committed to git — large, static
files fetched locally; see the root `.gitignore`
(`tts-models/en/*.onnx`, `*.onnx.json`) and the "voiceover setup" section
of the root README.

Run `npm run fetch-resources` to download these (and every other bundled
binary/model) in one shot from this repo's GitHub Release, or fetch them
manually per the README.

This file itself is just a placeholder so `resources/tts-models/` exists
in source control — `tauri_build` resolves the `resources/tts-models`
resource entry in `src-tauri/tauri.conf.json` at build time (including
`cargo test`), and an absent directory fails that resolution before any
test runs.
