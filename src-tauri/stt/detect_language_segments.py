"""Batched spoken-language identification across multiple time segments of
one audio file, using Systran/faster-whisper-tiny loaded once and run over
every segment in-process -- language ID is a much easier task than
transcription, so this tiny model is trustworthy for it despite being
useless for the real transcription job (Parakeet/the per-language Indic
Whisper fine-tunes do that). Loading it once here instead of spawning a
fresh process per segment matters because mixed_language.rs can have
dozens of short speech segments to classify per video. Used to classify
Tamil-vs-English (or any other supported language pair) per speech segment
before deciding how to group and transcribe them.

stdout: a single JSON line, a list of
`{"start": <float>, "end": <float>, "language": <code>, "probability": <float>}`
-- one entry per input segment, in the same order. On failure: an error
message on stderr and a nonzero exit.
"""

import argparse
import json
import sys

from faster_whisper import WhisperModel
from faster_whisper.audio import decode_audio

LANGUAGE_ID_MODEL = "Systran/faster-whisper-tiny"
SAMPLE_RATE = 16000


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("audio_path")
    parser.add_argument("--segments", required=True, help="path to a JSON file: [[start, end], ...]")
    args = parser.parse_args()

    try:
        with open(args.segments, "r", encoding="utf-8") as f:
            segments = json.load(f)

        if not segments:
            print(json.dumps([]))
            return

        model = WhisperModel(LANGUAGE_ID_MODEL, device="cpu", compute_type="int8")
        audio = decode_audio(args.audio_path, sampling_rate=SAMPLE_RATE)

        results = []
        for start, end in segments:
            start_sample = max(0, int(start * SAMPLE_RATE))
            end_sample = min(len(audio), int(end * SAMPLE_RATE))
            clip = audio[start_sample:end_sample]
            if len(clip) < SAMPLE_RATE * 0.2:
                # Too short to reliably identify -- the Rust caller's own
                # hysteresis step treats a low-enough-confidence result as
                # "inherit a neighboring segment's language" anyway, so
                # reporting zero confidence here (rather than guessing) is
                # exactly what should happen with a near-empty clip.
                results.append({"start": start, "end": end, "language": "unknown", "probability": 0.0})
                continue
            language, probability, _all_probs = model.detect_language(audio=clip)
            results.append({"start": start, "end": end, "language": language, "probability": probability})

        print(json.dumps(results))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"detect_language_segments failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
