"""English transcription via NVIDIA Parakeet (TDT 0.6B v2), run locally through
the onnx-asr package -- pure ONNX Runtime, no PyTorch/NeMo dependency, which is
why English gets its own lightweight script instead of sharing the heavier
faster-whisper+wav2vec2 stack the Indic path needs (see indic_transcribe.py).

Parakeet's own output is per-*token* (subword) start times, not word-level --
onnx_asr.TimestampedResult exposes parallel `tokens`/`timestamps` lists where a
token starting with a literal leading space marks the start of a new word
(SentencePiece-style; confirmed directly against this tokenizer's output, e.g.
[' A', 'pp', 'are', 'nt', 'ly', ','] -> "Apparently,"). Word timestamps are
reconstructed here the same way this project has merged sub-word ASR tokens
into words before (see captions.rs's own punctuation-merging logic): a word's
start is its first token's timestamp, and its end is approximated as the next
word's start (there is no explicit end time per token) -- or, for the last
word, the clip duration.

stdout: a single JSON line {"words": [{"word", "start", "end"}, ...], "language": "en"}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import sys

import onnx_asr
import soundfile as sf


def merge_tokens_into_words(tokens: list[str], timestamps: list[float], duration: float):
    words = []
    current_text = ""
    current_start = None

    def flush(end: float):
        nonlocal current_text, current_start
        text = current_text.strip()
        if text:
            words.append({"word": text, "start": current_start, "end": end})
        current_text = ""
        current_start = None

    for token, ts in zip(tokens, timestamps):
        starts_new_word = token.startswith(" ") or current_start is None
        if starts_new_word and current_text.strip():
            flush(ts)
        if current_start is None:
            current_start = ts
        current_text += token

    flush(duration)
    return words


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("audio_path")
    args = parser.parse_args()

    try:
        info = sf.info(args.audio_path)
        duration = info.frames / info.samplerate

        model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v2").with_timestamps()
        result = model.recognize(args.audio_path)

        words = merge_tokens_into_words(result.tokens, result.timestamps, duration)
        print(json.dumps({"words": words, "language": "en"}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"parakeet_transcribe failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
