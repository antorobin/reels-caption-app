# Converted Indic Whisper checkpoints

This directory (or `~/.reels-caption-app/stt-models/<language>/` as a
fallback — see `stt::indic_model_dir`) holds CTranslate2-converted,
int8-quantized Whisper checkpoints for Indian languages, one subdirectory
per language code (e.g. `ta/` for Tamil). Not committed to git — same
reasoning as the old whisper.cpp GGML models: large per-language binaries,
fetched/converted locally instead.

## Adding a language

Two sources work here, depending on how much RAM the converting machine has
free — CTranslate2's converter has to fully materialize the checkpoint in
memory before quantizing, so the smaller model matters on a memory-tight
laptop, not just for inference speed.

**Option A — AI4Bharat's IndicWhisper** (whisper-medium fine-tunes, ~1.4GB,
more accurate, needs ~2GB+ free RAM to convert): download from the
[Vistaar](https://github.com/AI4Bharat/vistaar) release table (a plain
HTTPS zip, no Hugging Face account/gating needed):
```bash
curl -L -o tamil_models.zip https://indicwhisper.objectstore.e2enetworks.net/tamil_models.zip
unzip tamil_models.zip -d tamil_models
conda run -n stt ct2-transformers-converter --model tamil_models/<extracted-checkpoint-dir> \
  --output_dir ta --quantization int8
```

**Option B — a smaller whisper-small fine-tune** (this is what Tamil
actually uses today, chosen after Option A repeatedly OOM'd on this
project's dev machine, which only had 0.3–2GB free RAM at the time):
[vasista22/whisper-tamil-small](https://huggingface.co/vasista22/whisper-tamil-small)
(IIT Madras, Apache 2.0, 244M params, not gated). `ct2-transformers-converter`
can pull it straight from the Hub, but its internal chunked download proved
flaky on a slow/unstable connection (repeatedly dropped in the last few % of
the ~967MB `pytorch_model.bin`); downloading the repo's files directly with
`curl -C -` (resumable) into a local directory first, then pointing the
converter at that local path, was reliable where the direct Hub download
wasn't:
```bash
curl -L -C - -o pytorch_model.bin https://huggingface.co/vasista22/whisper-tamil-small/resolve/main/pytorch_model.bin
# ...plus config.json, generation_config.json, preprocessor_config.json,
# tokenizer_config.json, vocab.json, merges.txt, normalizer.json,
# special_tokens_map.json, added_tokens.json — all small, fetch the same way.
conda run -n stt ct2-transformers-converter --model <local-dir-with-those-files> \
  --output_dir ta --quantization int8 --low_cpu_mem_usage
```

Then, regardless of which option was used:

3. Copy the resulting `ta/` directory here (or to
   `~/.reels-caption-app/stt-models/ta/`).
4. Add the language to `stt::INDIC_LANGUAGES` in `src-tauri/src/stt.rs`, and
   to the language dropdown in `src/components/shell/Inspector.jsx`. Word
   timestamps come directly from `faster-whisper`'s own native decoder
   output (`indic_transcribe.py`'s `word_timestamps=True`) — no separate
   per-language alignment model to pin. Do verify timestamp quality
   directly against real audio for the new language before trusting it —
   `chunk_length` (capping how much audio the decoder processes per
   internal pass) is what keeps these accurate, and a very different
   fine-tune could behave differently near that boundary.
