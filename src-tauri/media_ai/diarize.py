"""Lightweight speaker diarization heuristic — no pyannote/torch, no
account/token setup. Candidate speaker-turn segments come from the Rust
side (silence-gap boundaries, reusing jumpcuts.rs's own logic); this
script only clusters those segments into a small number of speakers by
their MFCC voice-print, via a hand-rolled numpy k-means (k=2 fixed for
v1 — deliberately not scikit-learn for one call site, and not a
silhouette-score-style auto-k search, which would be over-engineering
for a "heuristic" feature).

stdout: a single JSON line, a list of
`{"start": <float>, "end": <float>, "speaker_id": <int>}` — one entry per
input segment, in the same order. On failure: an error message on stderr
and a nonzero exit.
"""

import argparse
import json
import sys

import librosa
import numpy as np

N_MFCC = 13
SPEAKER_COUNT = 2


def kmeans(x, k, iters=50, seed=0):
    n = x.shape[0]
    if n <= k:
        return np.arange(n) % max(k, 1)

    rng = np.random.default_rng(seed)
    # k-means++ initialization
    centers = [x[rng.integers(n)]]
    for _ in range(k - 1):
        dists = np.min([np.sum((x - c) ** 2, axis=1) for c in centers], axis=0)
        total = dists.sum()
        probs = dists / total if total > 0 else np.full(n, 1.0 / n)
        centers.append(x[rng.choice(n, p=probs)])
    centers = np.array(centers)

    labels = np.zeros(n, dtype=int)
    for i in range(iters):
        dists = np.linalg.norm(x[:, None, :] - centers[None, :, :], axis=2)
        new_labels = np.argmin(dists, axis=1)
        if i > 0 and np.array_equal(new_labels, labels):
            labels = new_labels
            break
        labels = new_labels
        for k_i in range(k):
            pts = x[labels == k_i]
            if len(pts) > 0:
                centers[k_i] = pts.mean(axis=0)
    return labels


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", required=True)
    parser.add_argument("--segments", required=True, help="path to a JSON file: [[start, end], ...]")
    args = parser.parse_args()

    try:
        with open(args.segments, "r", encoding="utf-8") as f:
            segments = json.load(f)

        if not segments:
            print(json.dumps([]))
            return

        y, sr = librosa.load(args.audio, sr=16000, mono=True)

        features = []
        for start, end in segments:
            start_sample = max(0, int(start * sr))
            end_sample = min(len(y), int(end * sr))
            clip = y[start_sample:end_sample]
            if len(clip) < sr * 0.1:  # too short for a meaningful MFCC — pad with silence
                clip = np.pad(clip, (0, int(sr * 0.1) - len(clip)))
            mfcc = librosa.feature.mfcc(y=clip, sr=sr, n_mfcc=N_MFCC)
            features.append(mfcc.mean(axis=1))

        x = np.array(features)
        # Per-dimension z-score normalization — MFCC coefficients have very
        # different natural scales, which would otherwise dominate the
        # euclidean distance k-means uses.
        std = x.std(axis=0)
        std[std < 1e-9] = 1.0
        x_norm = (x - x.mean(axis=0)) / std

        k = min(SPEAKER_COUNT, len(segments))
        labels = kmeans(x_norm, k)

        out = [
            {"start": start, "end": end, "speaker_id": int(label)}
            for (start, end), label in zip(segments, labels)
        ]
        print(json.dumps(out))
    except Exception as e:  # noqa: BLE001 - surfaced via stderr, matching run_media_ai_script's error path
        print(str(e), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
