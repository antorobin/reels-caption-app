"""Persistent live-dictation transcription worker -- loads the language-ID
model (Systran/faster-whisper-tiny), Parakeet (English), and (if given) the
Tamil Whisper checkpoint ONCE at startup, then serves repeated requests
over stdin/stdout without paying model-load cost again per request.

Why this exists, and why it's separate from the one-shot scripts
(parakeet_transcribe.py, indic_transcribe.py, detect_language_segments.py):
live dictation re-transcribes a growing buffer every ~2.5s, and a fresh
Python process + fresh model load every single time was the actual reason
it never felt "live" -- easily several seconds of pure startup overhead per
update, dwarfing the ~2.5s cadence. The main batch pipeline
(pipeline.rs/mixed_language.rs) still uses the one-shot scripts directly
and is untouched by this -- it only pays that cost once per video anyway,
so it doesn't need a persistent process.

Protocol: one JSON object per line on stdin, one JSON object per line on
stdout, strictly request-then-response (the Rust side only ever has one
request in flight). First line of stdout once ready: {"ready": true}.

Requests:
  {"op": "detect_languages", "audio_path": str, "segments": [[start, end], ...]}
    -> {"results": [{"start": float, "end": float, "language": str, "probability": float}, ...]}
  {"op": "transcribe", "audio_path": str, "language": "en" | "ta"}
    -> {"words": [{"word": str, "start": float, "end": float}]}
Any request can instead get back {"error": str}, which does NOT end the
worker -- it keeps serving subsequent requests.
"""

import argparse
import json
import sys

import onnx_asr
import soundfile as sf
from faster_whisper import WhisperModel
from faster_whisper.audio import decode_audio

LANGUAGE_ID_MODEL = "Systran/faster-whisper-tiny"
SAMPLE_RATE = 16000
# Same reasoning as indic_transcribe.py: bounds how much audio Whisper's
# decoder processes in one internal pass, which is what actually keeps
# word-level timestamps accurate (not a VAD setting).
MAX_CHUNK_LENGTH_SECONDS = 15


def merge_parakeet_tokens_into_words(tokens, timestamps, duration):
    # Identical to parakeet_transcribe.py's merge_tokens_into_words --
    # kept in sync deliberately, see that script for the reasoning.
    words = []
    current_text = ""
    current_start = None

    def flush(end):
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


def handle_detect_languages(request, lang_id_model):
    audio_path = request["audio_path"]
    segments = request["segments"]
    audio = decode_audio(audio_path, sampling_rate=SAMPLE_RATE)

    results = []
    for start, end in segments:
        start_sample = max(0, int(start * SAMPLE_RATE))
        end_sample = min(len(audio), int(end * SAMPLE_RATE))
        clip = audio[start_sample:end_sample]
        if len(clip) < SAMPLE_RATE * 0.2:
            results.append({"start": start, "end": end, "language": "unknown", "probability": 0.0})
            continue
        language, probability, _all_probs = lang_id_model.detect_language(audio=clip)
        results.append({"start": start, "end": end, "language": language, "probability": probability})

    return {"results": results}


def handle_transcribe(request, parakeet_model, tamil_model):
    audio_path = request["audio_path"]
    language = request["language"]

    if language == "ta":
        if tamil_model is None:
            return {"error": "Tamil model wasn't loaded (no converted checkpoint was found at worker startup)."}
        segments, _info = tamil_model.transcribe(
            audio_path,
            language="ta",
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
        return {"words": words}

    if language == "en":
        info = sf.info(audio_path)
        duration = info.frames / info.samplerate
        result = parakeet_model.recognize(audio_path)
        words = merge_parakeet_tokens_into_words(result.tokens, result.timestamps, duration)
        return {"words": words}

    return {"error": f"Unsupported language for live transcription: {language!r}"}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--tamil-model-dir", default=None)
    args = parser.parse_args()

    print("Loading live-dictation models...", file=sys.stderr)
    lang_id_model = WhisperModel(LANGUAGE_ID_MODEL, device="cpu", compute_type="int8")
    parakeet_model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v2").with_timestamps()
    tamil_model = WhisperModel(args.tamil_model_dir, device="cpu", compute_type="int8") if args.tamil_model_dir else None
    print("Live-dictation models loaded.", file=sys.stderr)

    print(json.dumps({"ready": True}))
    sys.stdout.flush()

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
            op = request.get("op")
            if op == "detect_languages":
                response = handle_detect_languages(request, lang_id_model)
            elif op == "transcribe":
                response = handle_transcribe(request, parakeet_model, tamil_model)
            else:
                response = {"error": f"Unknown op: {op!r}"}
        except Exception as e:  # noqa: BLE001 -- kept alive for the next request either way
            response = {"error": str(e)}

        print(json.dumps(response))
        sys.stdout.flush()


if __name__ == "__main__":
    main()
