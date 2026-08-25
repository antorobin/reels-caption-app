"""Rough gender classification of a speaker from a reference audio clip --
median fundamental frequency (F0) via librosa.pyin, thresholded against the
typical adult male/female pitch-range boundary (~165Hz). Used only by
voice_clone.rs's gender-matched fallback tier, when real voice cloning
(OpenVoice's ToneColorConverter) isn't possible -- picks the closer-matching
bundled Piper voice instead of always defaulting to the same one.

prosody.py deliberately avoids librosa.pyin for its own per-word emphasis
detection (flaky on short, per-word windows) in favor of RMS energy. That
concern doesn't apply here: this runs once on a much longer (up to 30s)
reference clip, a far more forgiving case for pitch tracking than a
fraction-of-a-second word slice.

stdout: a single JSON line {"gender": "male"|"female"|null, "median_f0_hz": <float>|null}.
`gender` is null when too little voiced audio was found to get a confident
read (silence, music, noise) -- callers should treat that as "unknown" and
fall back to the default voice, not guess. On failure: an error message on
stderr and a nonzero exit (matching run_media_ai_script's error path).
"""

import argparse
import json
import sys

import librosa
import numpy as np

# Typical adult male F0 range is roughly 85-180Hz, female roughly
# 165-255Hz -- 165Hz sits at their overlap boundary, a standard
# voice-pitch heuristic split point.
MALE_FEMALE_F0_BOUNDARY_HZ = 165.0

# Below this, there isn't enough clean voiced signal to trust a median --
# rather than report a shaky guess, this is treated as "unknown".
MIN_VOICED_FRAMES = 20


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("audio_path")
    args = parser.parse_args()

    try:
        y, sr = librosa.load(args.audio_path, sr=16000, mono=True)
        f0, voiced_flag, _voiced_prob = librosa.pyin(
            y, fmin=librosa.note_to_hz("C2"), fmax=librosa.note_to_hz("C6"), sr=sr
        )
        voiced_f0 = f0[voiced_flag & ~np.isnan(f0)]

        if len(voiced_f0) < MIN_VOICED_FRAMES:
            print(json.dumps({"gender": None, "median_f0_hz": None}))
            return

        median_f0 = float(np.median(voiced_f0))
        gender = "male" if median_f0 < MALE_FEMALE_F0_BOUNDARY_HZ else "female"
        print(json.dumps({"gender": gender, "median_f0_hz": median_f0}))
    except Exception as e:  # noqa: BLE001 - surfaced via stderr, matching run_media_ai_script's error path
        print(str(e), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
