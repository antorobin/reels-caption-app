# Minimal ffmpeg / ffprobe build

The bundled `src-tauri/resources/bin/ffmpeg.exe` and `ffprobe.exe` are
**242 MB each** today because they are full "all the things" static
builds. Together that is ~484 MB of the installer. A build restricted to
what this app actually invokes is **~35-45 MB each**.

The one non-standard requirement is **`librubberband`** — the emotion
pitch-shift in `tts.rs` (`emotion_preset`) uses ffmpeg's `rubberband`
filter for formant-preserving pitch change, and stock prebuilt ffmpeg
(gyan.dev / BtbN "essentials" or "full") does **not** include it. That is
the reason a stock small build can't just be dropped in.

## What the app uses (verified against the source)

Grep the tree before changing this list — every item below is reached
from `src-tauri/src/*.rs` or a `media_ai` / `stt` / `tts` Python script.

**Video filters:** `ass`, `subtitles`, `scale`, `crop`, `fade`, `eq`,
`fps`, `zoompan`, `format`, `overlay`, `setpts`, `concat`, `colorchannelmixer`
**Audio filters:** `loudnorm`, `silenceremove`, `silencedetect`, `afftdn`,
`highpass`, `rubberband`, `volume`, `adelay`, `amix`, `apad`, `atrim`,
`aformat`, `aresample`, `anull`, `asetpts`, `pan`
**Encoders:** `libx264`, `aac` (native), `pcm_s16le`; optional HW:
`h264_nvenc` (probed at runtime in `ffmpeg.rs`, falls back to libx264)
**Decoders:** `h264`, `hevc`, `vp8`, `vp9`, `mpeg4`, `mjpeg`, `aac`,
`mp3`, `pcm_*`, `flac`, `opus`, `vorbis`
**Demux/mux:** `mov,mp4,m4a`, `matroska,webm`, `avi`, `wav`, `mp3`,
`image2`, `aac`, `ipod`
**Protocols:** `file`, `pipe`
**Parsers / bitstream filters:** `h264_mp4toannexb`, `aac_adtstoasc`,
`extract_extradata`

## configure flags

Start from `--disable-everything` and add back exactly the above:

```
--disable-everything --disable-doc --disable-ffplay --disable-debug \
--disable-network --disable-sdl2 --disable-autodetect \
--enable-gpl --enable-version3 \
--enable-libx264 --enable-libass --enable-librubberband \
--enable-encoder=libx264,aac,pcm_s16le,pcm_f32le \
--enable-decoder=h264,hevc,vp8,vp9,mpeg4,mjpeg,aac,mp3,flac,opus,vorbis,pcm_s16le,pcm_s16be,pcm_f32le,pcm_u8 \
--enable-demuxer=mov,matroska,avi,wav,mp3,aac,image2,flac,ogg \
--enable-muxer=mp4,mov,matroska,webm,wav,mp3,aac,image2,ipod,null \
--enable-parser=h264,hevc,aac,vp8,vp9,mpeg4video,mjpeg,flac,opus,vorbis \
--enable-bsf=h264_mp4toannexb,aac_adtstoasc,extract_extradata \
--enable-protocol=file,pipe \
--enable-filter=ass,subtitles,scale,crop,fade,eq,fps,zoompan,format,overlay,setpts,concat,colorchannelmixer,aresample,aformat,anull,asetpts,loudnorm,silenceremove,silencedetect,afftdn,highpass,volume,adelay,amix,apad,atrim,pan,rubberband \
--enable-filter=hwdownload,hwupload,null,copy,trim \
--enable-small
```

Add `--enable-nvenc --enable-ffnvcodec` (headers only, no CUDA SDK) to
keep the `h264_nvenc` fast path; it costs almost nothing in size.

`ffprobe` comes from the same build tree — it should be ~1-2 MB, not
242 MB. If your build produces a fat ffprobe, the build is statically
linking the whole libav* set into it; use `--enable-shared` +
ship the DLLs, or accept two copies of a *small* static binary.

## How to actually build it

Cross-compiling ffmpeg with these libs from scratch is a project. Use one
of:

1. **[BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds)** — fork
   it, edit `scripts.d/` to add `librubberband` and swap the addins list
   for `--disable-everything` + the flags above, run its Docker build.
   This produces a `win64` static binary. Least fuss.
2. **[media-autobuild_suite](https://github.com/m-ab-s/media-autobuild_suite)**
   on a Windows box — interactive, pick `librubberband`, `libass`,
   `libx264`, paste the custom configure line when prompted.
3. A GitHub Actions job (recommended for repeatability) — run option 1's
   Docker image in CI, upload `ffmpeg.exe` / `ffprobe.exe` as artifacts,
   commit them to `src-tauri/resources/bin/` (or attach to a Release and
   fetch via `scripts/fetch-dev-resources.mjs`).

## Verify before shipping

```
ffmpeg -hide_banner -filters   | grep -E 'ass|rubberband|loudnorm|silenceremove|zoompan|afftdn'
ffmpeg -hide_banner -encoders  | grep -E 'libx264|aac'
```

Then run the app's real paths end to end: a caption burn (ass), an
emotion-preset voiceover (rubberband), silence removal (silenceremove),
a transition (zoompan/eq/fade), music ducking (amix/adelay/volume).
