"""Spoken-language identification for an audio file -- Systran's
pre-converted CTranslate2 build of Whisper-tiny (`Systran/faster-whisper-tiny`,
downloaded on demand and cached, same as the Indic Whisper checkpoints),
used purely for its `detect_language()` call, never for actual
transcription (tiny is far too small for that -- Parakeet/the per-language
Indic Whisper fine-tunes do the real work in parakeet_transcribe.py /
indic_transcribe.py).

Verified directly on this project's own test audio: 98.7% confidence on
English, 91.9% on Tamil (second guess Malayalam at 6%, a related Dravidian
script -- still an unambiguous top pick). Language ID is a much easier
task than transcription, which is why a model this small is trustworthy
here despite being useless for the transcription job itself.

stdout: a single JSON line {"language": <ISO 639-1 code>, "probability": <float>}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import sys

from faster_whisper import WhisperModel
from faster_whisper.audio import decode_audio

LANGUAGE_ID_MODEL = "Systran/faster-whisper-tiny"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("audio_path")
    args = parser.parse_args()

    try:
        model = WhisperModel(LANGUAGE_ID_MODEL, device="cpu", compute_type="int8")
        audio = decode_audio(args.audio_path, sampling_rate=16000)
        language, probability, _all_probs = model.detect_language(audio=audio)
        print(json.dumps({"language": language, "probability": probability}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"detect_language failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
