"""Voice-over <-> mouth-movement cadence sync (v1: constant-offset only).

Tracks a mouth-aspect-ratio (MAR) curve from the video via mediapipe
FaceMesh, computes the voice-over's onset-strength envelope via librosa,
cross-correlates the two to find the best constant time offset, then
muxes the voice-over onto the video at that offset via ffmpeg.

Deliberately offset-only, not variable-rate time-stretch/DTW: this is a
standalone tool for "my dubbed audio is a beat off from the talking
cadence," not word-perfect generative lip-sync (which needs a real
vision/generative model). If the voice-over's pacing drifts relative to
the video over time, only the initial offset is corrected.

stdout: a single JSON line `{"offset_seconds": <float>, "output_path": "<path>"}`
on success, or `{"error": "<message>"}` (nonzero exit) on failure.
"""

import argparse
import json
import subprocess
import sys

import cv2
import librosa
import numpy as np
from scipy.signal import correlate

# mediapipe FaceMesh landmark indices for a simple mouth-aspect-ratio:
# vertical inner-lip gap over horizontal mouth-corner distance.
UPPER_LIP = 13
LOWER_LIP = 14
LEFT_CORNER = 61
RIGHT_CORNER = 291

MOUTH_SAMPLE_FPS = 8.0  # full 30fps landmark tracking is unnecessary and slow for this
COMMON_GRID_RATE = 25.0
# scipy.signal.correlate's 'full' mode is unnormalized: near the edges of
# the overlap window, only a handful of samples contribute to each lag's
# sum, so a few coincidentally-aligned samples there can outscore the true
# (much longer-overlap, but proportionally smaller-sum) peak near lag 0 —
# confirmed empirically (a real ~50s clip with a genuine 2s offset
# produced a spurious 36s "best" lag before this bound was added). Realistic
# voice-over cadence drift for a short-form clip is nowhere near tens of
# seconds, so restricting the search window is both a correctness fix and
# a reasonable scope choice for what this tool is for.
MAX_OFFSET_SECONDS = 12.0


def compute_mouth_curve(video_path):
    import mediapipe as mp

    cap = cv2.VideoCapture(video_path)
    if not cap.isOpened():
        raise RuntimeError(f"Could not open video: {video_path}")
    src_fps = cap.get(cv2.CAP_PROP_FPS) or 30.0
    frame_interval = max(1, round(src_fps / MOUTH_SAMPLE_FPS))

    times, mars = [], []
    mp_face_mesh = mp.solutions.face_mesh
    with mp_face_mesh.FaceMesh(
        static_image_mode=False, max_num_faces=1, min_detection_confidence=0.5, min_tracking_confidence=0.5
    ) as face_mesh:
        frame_idx = 0
        while True:
            ok, frame = cap.read()
            if not ok:
                break
            if frame_idx % frame_interval == 0:
                rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
                result = face_mesh.process(rgb)
                if result.multi_face_landmarks:
                    lm = result.multi_face_landmarks[0].landmark
                    h, w = frame.shape[:2]
                    upper = np.array([lm[UPPER_LIP].x * w, lm[UPPER_LIP].y * h])
                    lower = np.array([lm[LOWER_LIP].x * w, lm[LOWER_LIP].y * h])
                    left = np.array([lm[LEFT_CORNER].x * w, lm[LEFT_CORNER].y * h])
                    right = np.array([lm[RIGHT_CORNER].x * w, lm[RIGHT_CORNER].y * h])
                    horizontal = np.linalg.norm(left - right)
                    if horizontal > 1e-6:
                        mar = np.linalg.norm(upper - lower) / horizontal
                        times.append(frame_idx / src_fps)
                        mars.append(mar)
            frame_idx += 1
    cap.release()
    return np.array(times), np.array(mars)


def compute_voiceover_envelope(voiceover_path):
    y, sr = librosa.load(voiceover_path, sr=16000, mono=True)
    onset_env = librosa.onset.onset_strength(y=y, sr=sr)
    times = librosa.times_like(onset_env, sr=sr)
    return times, onset_env


def find_offset_seconds(mouth_times, mouth_vals, vo_times, vo_vals):
    """Positive result: delay the voice-over (it should start later relative
    to the video). Negative: advance it (start earlier)."""
    if len(mouth_times) < 2 or len(vo_times) < 2:
        return 0.0
    duration = min(mouth_times[-1], vo_times[-1])
    if duration <= 0:
        return 0.0

    grid = np.arange(0, duration, 1.0 / COMMON_GRID_RATE)
    a = np.interp(grid, mouth_times, mouth_vals)
    b = np.interp(grid, vo_times, vo_vals)
    a = (a - a.mean()) / (a.std() + 1e-9)
    b = (b - b.mean()) / (b.std() + 1e-9)

    corr = correlate(a, b, mode="full")
    lags = np.arange(-(len(b) - 1), len(a))

    max_lag_samples = int(MAX_OFFSET_SECONDS * COMMON_GRID_RATE)
    in_range = np.abs(lags) <= max_lag_samples
    corr, lags = corr[in_range], lags[in_range]

    best_lag = lags[int(np.argmax(corr))]
    return float(best_lag / COMMON_GRID_RATE)


def mux(ffmpeg_path, video_path, voiceover_path, offset_seconds, out_path):
    args = [
        ffmpeg_path,
        "-y",
        "-i", video_path,
        "-itsoffset", str(offset_seconds),
        "-i", voiceover_path,
        "-map", "0:v:0",
        "-map", "1:a:0",
        "-c:v", "copy",
        "-c:a", "aac",
        "-shortest",
        out_path,
    ]
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        raise RuntimeError(f"ffmpeg mux failed: {result.stderr[-2000:]}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--video", required=True)
    parser.add_argument("--voiceover", required=True)
    parser.add_argument("--ffmpeg", help="required unless --offset-only")
    parser.add_argument("--out", help="required unless --offset-only")
    parser.add_argument(
        "--offset-only",
        action="store_true",
        help="Just compute+print the offset, skip the ffmpeg mux -- for live in-browser preview sync "
        "(see VideoPreview.jsx), which plays the voiceover alongside the original video file directly "
        "rather than needing a separately-muxed output file.",
    )
    args = parser.parse_args()

    try:
        mouth_times, mouth_vals = compute_mouth_curve(args.video)
        if len(mouth_times) == 0:
            raise RuntimeError("No face/mouth detected in the video — can't sync against mouth movement.")
        vo_times, vo_vals = compute_voiceover_envelope(args.voiceover)
        offset_seconds = find_offset_seconds(mouth_times, mouth_vals, vo_times, vo_vals)
        if args.offset_only:
            print(json.dumps({"offset_seconds": offset_seconds}))
            return
        if not args.ffmpeg or not args.out:
            raise RuntimeError("--ffmpeg and --out are required unless --offset-only is set")
        mux(args.ffmpeg, args.video, args.voiceover, offset_seconds, args.out)
        print(json.dumps({"offset_seconds": offset_seconds, "output_path": args.out}))
    except Exception as e:  # noqa: BLE001 - surfaced to the caller via stderr, matching run_media_ai_script's error path
        print(str(e), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
