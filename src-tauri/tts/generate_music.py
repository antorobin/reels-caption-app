"""Instrumental background-music generation via Meta's MusicGen-Small
(`facebook/musicgen-small`, official Hugging Face `transformers` model) --
text-conditioned, CPU-capable, 300M params. Lives in the "tts" env rather
than a dedicated one: that env already has `torch`+`transformers`+`scipy`
(see tts.rs's own module doc comment) for MMS-TTS, which is exactly what
this needs too -- no new conda environment for one more model.

TinyMusician (a smaller, faster distillation of MusicGen) was considered
first but has no public checkpoint, pip package, or GitHub implementation
as of this writing (Sept 2025 arXiv paper only) -- MusicGen-Small is the
actual usable option today.

`facebook/musicgen-small` itself is fetched on first use via `transformers`'
own Hugging Face Hub cache (~1.2GB) -- the same lazy-download behavior
already relied on elsewhere in this app (e.g. `WhisperModel(LANGUAGE_ID_MODEL)`
in detect_language_segments.py), not a new download mechanism.

`--duration` is converted to `max_new_tokens` via MusicGen's own documented
figure of ~50 audio tokens/second (its EnCodec-based tokenizer's frame
rate) -- this is what actually controls how much audio comes out, not a
post-hoc trim.

stdout: a single JSON line {"output_path": <str>, "sample_rate": <int>}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import sys

TOKENS_PER_SECOND = 50


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--prompt", required=True, help="Text description of the instrumental style to generate")
    parser.add_argument("--duration", type=float, required=True, help="Target audio length in seconds")
    parser.add_argument("--output", required=True, help="Output .wav path")
    args = parser.parse_args()

    try:
        import scipy.io.wavfile
        import torch
        from transformers import AutoProcessor, MusicgenForConditionalGeneration

        model_id = "facebook/musicgen-small"
        processor = AutoProcessor.from_pretrained(model_id)
        model = MusicgenForConditionalGeneration.from_pretrained(model_id)
        model.eval()

        inputs = processor(text=[args.prompt], padding=True, return_tensors="pt")
        max_new_tokens = max(1, int(args.duration * TOKENS_PER_SECOND))

        with torch.no_grad():
            audio_values = model.generate(**inputs, max_new_tokens=max_new_tokens, do_sample=True, guidance_scale=3.0)

        sample_rate = model.config.audio_encoder.sampling_rate
        audio = audio_values[0, 0].cpu().numpy()
        scipy.io.wavfile.write(args.output, rate=sample_rate, data=audio)

        print(json.dumps({"output_path": args.output, "sample_rate": sample_rate}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"generate_music failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
