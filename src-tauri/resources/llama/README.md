# Bundled llama.cpp binaries + Qwen2.5-0.5B-Instruct GGUF

This directory holds the `llama-server` executable (plus any DLLs it needs
on Windows) and the `Qwen2.5-0.5B-Instruct` GGUF model that back the local
LLM service. Not committed to git — large, static binaries that never
change once fetched; see the root `.gitignore` (`llama/*.exe`, `*.dll`,
`*.gguf`) and the "LLM service setup" section of the root README.

Run `npm run fetch-resources` to download these (and every other bundled
binary/model) in one shot from this repo's GitHub Release, or fetch them
manually per the README.

This file itself is just a placeholder so `resources/llama/` exists in
source control — `tauri_build` resolves the `resources/llama/*` glob in
`src-tauri/tauri.conf.json` at build time (including `cargo test`), and an
absent directory fails that resolution before any test runs.
