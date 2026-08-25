"""Indian-language text-to-speech via Meta's MMS-TTS (Massively Multilingual
Speech) -- per-language VITS checkpoints, same lightweight model family as
Piper, downloaded on demand from Hugging Face (`facebook/mms-tts-<code>`,
cached under ~/.cache/huggingface after the first run -- no manual
conversion step needed, unlike the STT models in resources/stt-models/).

Runs through `transformers`' VitsModel + torch (CPU), not onnxruntime --
verified directly: unlike Piper, this doesn't speed up after a warm-up
call, staying at roughly 3x slower than real-time on this project's dev
machine (~10s to synthesize ~3.4s of audio). Still fully usable for
generating a voiceover file (not a live/interactive use case), and uses
Meta's own officially-published checkpoints rather than a
community-converted ONNX export of uncertain freshness -- the same
reasoning that favored an official GGUF over a third-party quant
elsewhere in this project (see llm.rs).

MMS-TTS keys its per-language checkpoints by ISO 639-3 code (e.g. "tam"
for Tamil), not the ISO 639-1 codes ("ta") this app's `stt::INDIC_LANGUAGES`
uses elsewhere -- LANGUAGE_TO_MMS_CODE bridges that.

stdout: a single JSON line {"output_path": <str>}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import sys

import scipy.io.wavfile
import torch
from transformers import AutoTokenizer, VitsModel

LANGUAGE_TO_MMS_CODE = {
    "ta": "tam",
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("text")
    parser.add_argument("--language", required=True, help="ISO 639-1 code, e.g. ta")
    parser.add_argument("--output", required=True, help="Output .wav path")
    parser.add_argument(
        "--speaking-rate", type=float, default=None, help=">1 faster, <1 slower (default: model's own, 1.0)"
    )
    args = parser.parse_args()

    mms_code = LANGUAGE_TO_MMS_CODE.get(args.language)
    if mms_code is None:
        print(f"mms_synthesize failed: no MMS-TTS mapping for language '{args.language}'", file=sys.stderr)
        sys.exit(1)

    try:
        model_id = f"facebook/mms-tts-{mms_code}"
        model = VitsModel.from_pretrained(model_id)
        tokenizer = AutoTokenizer.from_pretrained(model_id)
        if args.speaking_rate is not None:
            model.config.speaking_rate = args.speaking_rate

        inputs = tokenizer(args.text, return_tensors="pt")
        with torch.no_grad():
            waveform = model(**inputs).waveform

        scipy.io.wavfile.write(args.output, rate=model.config.sampling_rate, data=waveform.float().numpy().T)
        print(json.dumps({"output_path": args.output}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"mms_synthesize failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
