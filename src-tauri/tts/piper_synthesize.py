"""English text-to-speech via Piper -- a small (~63MB), ONNX-based neural
TTS built specifically for fast CPU/edge inference (same `onnxruntime`
story as this project's Parakeet setup). The first synthesis call in a
fresh process pays a one-time ONNX Runtime graph-optimization cost
(confirmed directly: ~10s for a one-sentence clip); a warm process is
close to real-time. Since this script is a one-shot subprocess (matching
this project's other STT/TTS scripts), every call pays that cost -- the
same tradeoff already accepted elsewhere in this app for jobs that take
noticeable time (transcription, caption burning).

stdout: a single JSON line {"output_path": <str>}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import sys
import wave

from piper import PiperVoice
from piper.config import SynthesisConfig


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("text")
    parser.add_argument("--model-path", required=True, help="Path to a Piper voice's .onnx file")
    parser.add_argument("--output", required=True, help="Output .wav path")
    parser.add_argument(
        "--length-scale", type=float, default=None, help="VITS length_scale: >1 slower, <1 faster (default: model's own)"
    )
    args = parser.parse_args()

    try:
        voice = PiperVoice.load(args.model_path)
        syn_config = SynthesisConfig(length_scale=args.length_scale) if args.length_scale is not None else None
        with wave.open(args.output, "wb") as wav_file:
            voice.synthesize_wav(args.text, wav_file, syn_config=syn_config)
        print(json.dumps({"output_path": args.output}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"piper_synthesize failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
