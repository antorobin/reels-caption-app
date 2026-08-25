"""Voice cloning: reshapes `--source`'s timbre to match the speaker in
`--reference`, via OpenVoice V2's ToneColorConverter
(https://github.com/myshell-ai/OpenVoice, MIT license). `--source` is
already-synthesized voiceover audio (Piper/MMS-TTS, from this app's own
tts.rs); `--reference` is a clip of the original video's own speaker
(voice_clone.rs's extract_reference_clip). Only acoustic timbre changes --
content and language are untouched, which is why this works the same way
for Tamil source audio as English: the converter operates on the source
audio's own linguistic content, whatever language it's in.

This is the standard "clone any source audio" recipe OpenVoice's own docs
and demo notebooks describe (source_se + target_se via se_extractor.get_se,
then ToneColorConverter.convert) -- the exact call shape gets verified
directly against the installed package version the first time this runs
(see the README's voice-clone setup section for that verification step),
not assumed from docs alone.

stdout: a single JSON line {"output_path": <str>}.
On failure: an error message on stderr and a nonzero exit.
"""

import argparse
import json
import shutil
import sys
import tempfile

from openvoice import se_extractor
from openvoice.api import ToneColorConverter


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True, help="Already-synthesized voiceover WAV")
    parser.add_argument("--reference", required=True, help="Reference clip of the target speaker")
    parser.add_argument("--checkpoint-dir", required=True, help="Dir containing config.json + checkpoint.pth")
    parser.add_argument("--output", required=True, help="Output WAV path")
    args = parser.parse_args()

    # se_extractor.get_se() defaults to caching VAD-split segments + speaker
    # embeddings under a *relative* "processed/" directory -- resolved
    # against whatever the current process's CWD happens to be, not
    # anything under this script's own path. Confirmed the hard way: a
    # real "processed/" directory full of scratch audio/embeddings ended up
    # inside this project's own working tree (and got committed) before
    # this was pinned to an explicit temp directory. Cleaned up in
    # `finally` since nothing downstream needs these intermediate files.
    cache_dir = tempfile.mkdtemp(prefix="openvoice-processed-")

    try:
        converter = ToneColorConverter(f"{args.checkpoint_dir}/config.json", device="cpu")
        converter.load_ckpt(f"{args.checkpoint_dir}/checkpoint.pth")

        source_se, _ = se_extractor.get_se(args.source, converter, target_dir=cache_dir, vad=True)
        target_se, _ = se_extractor.get_se(args.reference, converter, target_dir=cache_dir, vad=True)

        converter.convert(
            audio_src_path=args.source,
            src_se=source_se,
            tgt_se=target_se,
            output_path=args.output,
        )
        print(json.dumps({"output_path": args.output}))
    except Exception as e:  # noqa: BLE001 -- surfaced to the Rust caller as a plain error message
        print(f"clone_voice failed: {e}", file=sys.stderr)
        sys.exit(1)
    finally:
        shutil.rmtree(cache_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
