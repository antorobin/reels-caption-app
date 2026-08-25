"""Prosody-driven per-word vocal emphasis — a non-vision replacement for
the deleted vision-based emotion caption emphasis. Uses RMS energy (loud
= emphasized) rather than pitch tracking (librosa.pyin is comparatively
slow/flaky on short windows and unvoiced speech) — a simpler, more robust
signal for "is this word vocally emphasized."

Buckets are percentile-based (this clip's own distribution), not a fixed
dB threshold — a fixed threshold wouldn't generalize across differently
mixed/normalized source audio, whereas percentiles adapt per-clip.

stdout: a single JSON line, a list of
`{"start": <float>, "end": <float>, "intensity": "low"|"medium"|"high"}`
aligned 1:1 with the input word list. On failure: an error message on
stderr and a nonzero exit (matching run_media_ai_script's error path).
"""

import argparse
import json
import sys

import librosa
import numpy as np

RMS_FRAME_LENGTH = 1024
RMS_HOP_LENGTH = 256


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", required=True)
    parser.add_argument("--words", required=True, help="path to a JSON file: [{word,start,end}, ...]")
    args = parser.parse_args()

    try:
        with open(args.words, "r", encoding="utf-8") as f:
            words = json.load(f)

        if not words:
            print(json.dumps([]))
            return

        y, sr = librosa.load(args.audio, sr=16000, mono=True)
        rms = librosa.feature.rms(y=y, frame_length=RMS_FRAME_LENGTH, hop_length=RMS_HOP_LENGTH)[0]
        times = librosa.frames_to_time(np.arange(len(rms)), sr=sr, hop_length=RMS_HOP_LENGTH)

        peaks = []
        for w in words:
            start, end = w["start"], w["end"]
            mask = (times >= start) & (times < max(end, start + 0.02))
            if np.any(mask):
                peaks.append(float(rms[mask].max()))
            else:
                idx = int(np.argmin(np.abs(times - start)))
                peaks.append(float(rms[idx]))

        peaks_arr = np.array(peaks)
        low_thresh = np.percentile(peaks_arr, 33)
        high_thresh = np.percentile(peaks_arr, 67)

        out = []
        for w, peak in zip(words, peaks):
            if peak >= high_thresh and high_thresh > low_thresh:
                intensity = "high"
            elif peak <= low_thresh:
                intensity = "low"
            else:
                intensity = "medium"
            out.append({"start": w["start"], "end": w["end"], "intensity": intensity})

        print(json.dumps(out))
    except Exception as e:  # noqa: BLE001 - surfaced via stderr, matching run_media_ai_script's error path
        print(str(e), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
