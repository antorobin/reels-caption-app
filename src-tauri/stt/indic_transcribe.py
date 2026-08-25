"""Indian-language transcription: faster-whisper (CTranslate2, int8) running a
per-language Indic Whisper fine-tune, with word-level timestamps taken
directly from its own native decoder output.

This used to forced-align the transcript against a separate wav2vec2 CTC
model via WhisperX's `align()` function, on the theory that Whisper's native
cross-attention timestamps drift on long-form audio (a real, confirmed
problem this project hit before, on a different model). That approach was
verified end-to-end on real Tamil audio and looked fine at first glance
(monotonically increasing timestamps) -- but real usage surfaced two bugs
that tracing back to the source: `faster-whisper` was producing occasional
huge (20-30s), unsplit segments for this model, and forced-aligning a giant
block of un-punctuated text against that much dense speech is exactly where
CTC alignment falls apart -- both the reported misaligned highlighting *and*
the reported "missing" words (they were never dropped by the ASR; they just
failed to get a placeable timestamp during alignment and were silently
filtered out downstream).

The actual fix is `chunk_length` (bounds how much audio Whisper's decoder
ever looks at in one internal pass -- NOT `vad_parameters`, which only
controls what survives the pre-decode silence-stripping pass and has no
effect on decode segment length). Capping it keeps every segment short
enough that native word timestamps stay accurate, verified directly: with
chunk_length=15, every word timestamp across a real 60s Tamil clip landed
under 1.1s in duration, versus an 11-second single-word artifact in the
uncapped version. This also means WhisperX and NLTK are no longer
dependencies of this script.

stdout: a single JSON line {"words": [{"word","start","end"}, ...], "language": <code>}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import sys

from faster_whisper import WhisperModel

# Caps how much audio Whisper's decoder processes in one internal pass --
# see module doc comment for why this (not a VAD setting) is what actually
# bounds segment/word-timestamp drift.
MAX_CHUNK_LENGTH_SECONDS = 15


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("audio_path")
    parser.add_argument("--language", required=True, help="ISO 639-1 code, e.g. ta, hi")
    parser.add_argument("--model-dir", required=True, help="CTranslate2-converted Indic Whisper checkpoint directory")
    args = parser.parse_args()

    try:
        model = WhisperModel(args.model_dir, device="cpu", compute_type="int8")
        segments, info = model.transcribe(
            args.audio_path,
            language=args.language,
            vad_filter=True,
            chunk_length=MAX_CHUNK_LENGTH_SECONDS,
            word_timestamps=True,
        )

        words = [
            {"word": w.word.strip(), "start": w.start, "end": w.end}
            for s in segments
            for w in (s.words or [])
            if w.word.strip()
        ]
        print(json.dumps({"words": words, "language": args.language}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"indic_transcribe failed: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
