// Packs the built runtime pieces into the tarballs the app's
// `runtime_fetch.rs` downloads, computes their SHA-256s, and writes a
// filled-in `components.json` for upload to the GitHub Release.
//
//   node scripts/build-python-runtime.mjs --pack=all   # produce resources/python/*
//   npm run fetch-resources                            # resources/llama, resources/tts-models
//   # ...put a librubberband-capable ffmpeg/ffprobe in resources/bin/
//   node scripts/pack-components.mjs --out dist/components --version 2026.1
//
// Output (dist/components/):
//   ffmpeg-win-x64.tar.gz          <- resources/bin/{ffmpeg,ffprobe}.exe + *.dll
//   python-stt-win-x64.tar.gz      <- resources/python/stt/            (archive root: stt/)
//   python-voice-win-x64.tar.gz    <- resources/python/{tts,media-ai,voice-clone}/ + resources/tts-models/en/
//   llm-qwen-0.5b-win-x64.tar.gz   <- resources/llama/*
//   components.json                <- src-tauri/components.json with url/sha256/size/version filled
//
// CI uploads all of these as assets on the same Release the updater reads.
// Pure Node (no `rm`/`cp` shell-outs) so it runs on the windows-latest
// runner as-is; only `tar` is external (bsdtar ships with Windows 10+).

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { cpSync, createReadStream, existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(HERE, "..");
const SRC_TAURI = join(ROOT, "src-tauri");
const RES = join(SRC_TAURI, "resources");

const argv = process.argv.slice(2);
const args = {};
for (let i = 0; i < argv.length; i++) {
  if (!argv[i].startsWith("--")) continue;
  const k = argv[i].slice(2);
  args[k] = argv[i + 1] && !argv[i + 1].startsWith("--") ? argv[++i] : true;
}

const OUT = resolve(ROOT, args.out || "dist/components");
const VERSION = args.version || new Date().toISOString().slice(0, 10);
const RELEASE_BASE =
  args["release-base"] || "https://github.com/antorobin/reels-caption-app/releases/latest/download";
// Which platform's entries to (re)build. The manifest carries one entry
// per (id, platform); this run touches only the matching ones.
const PLATFORM =
  args.platform ||
  { win32: "windows-x64", darwin: process.arch === "arm64" ? "macos-arm64" : "macos-x64", linux: "linux-x64" }[
    process.platform
  ];

// id -> pack recipe. `base` + `-<platform>.tar.gz` is the asset name.
// `entries` are relative to `cwd`; archive contents land at the archive
// root so `runtime_fetch`'s `unpack_to` maps cleanly.
const PACKS = {
  ffmpeg: { base: "ffmpeg", cwd: join(RES, "bin"), entries: ["."] },
  "python-stt": { base: "python-stt", cwd: join(RES, "python"), entries: ["stt"] },
  "python-voice": {
    base: "python-voice",
    // two source roots -> stage a temp tree so the archive is one flat unpack
    staged: [
      [join(RES, "python"), ["tts", "media-ai", "voice-clone"]],
      [RES, [join("tts-models", "en")]],
    ],
    archiveRootEntries: ["python", "tts-models"],
  },
  llm: { base: "llm-qwen-0.5b", cwd: join(RES, "llama"), entries: ["."] },
};

function sha256(file) {
  return new Promise((res, rej) => {
    const h = createHash("sha256");
    createReadStream(file).on("data", (d) => h.update(d)).on("end", () => res(h.digest("hex"))).on("error", rej);
  });
}

function tar(archivePath, cwd, entries) {
  execFileSync("tar", ["-czf", archivePath, "-C", cwd, ...entries], { stdio: "inherit" });
}

mkdirSync(OUT, { recursive: true });
const manifestPath = join(SRC_TAURI, "components.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

// The committed manifest only lists windows-x64. For another platform,
// synthesize its entries from the windows ones (same id/tier/unpack_to/
// needed_for; url/sha256/size get filled below).
if (!manifest.components.some((c) => c.platform === PLATFORM)) {
  const seed = manifest.components.filter((c) => c.platform === "windows-x64");
  for (const c of seed) {
    manifest.components.push({ ...c, platform: PLATFORM, url: "", sha256: "", size: 0 });
  }
}

console.log(`Packing platform: ${PLATFORM}`);
for (const c of manifest.components) {
  if (c.platform !== PLATFORM) continue;
  const pack = PACKS[c.id];
  if (!pack) {
    console.warn(`! no pack recipe for component "${c.id}" — skipping`);
    continue;
  }
  const archiveName = `${pack.base}-${PLATFORM}.tar.gz`;
  const archivePath = join(OUT, archiveName);

  if (pack.staged) {
    const stage = join(OUT, `_stage-${c.id}`);
    rmSync(stage, { recursive: true, force: true });
    for (const [srcRoot, dirs] of pack.staged) {
      for (const d of dirs) {
        const from = join(srcRoot, d);
        if (!existsSync(from)) throw new Error(`missing ${from} — build the runtime / fetch resources first`);
        const to = join(stage, d);
        mkdirSync(dirname(to), { recursive: true });
        cpSync(from, to, { recursive: true });
      }
    }
    tar(archivePath, stage, pack.archiveRootEntries);
    rmSync(stage, { recursive: true, force: true });
  } else {
    if (!existsSync(pack.cwd)) throw new Error(`missing ${pack.cwd} — build the runtime / fetch resources first`);
    tar(archivePath, pack.cwd, pack.entries);
  }

  const digest = await sha256(archivePath);
  const size = statSync(archivePath).size;
  c.version = VERSION;
  c.url = `${RELEASE_BASE}/${archiveName}`;
  c.sha256 = digest;
  c.size = size;
  console.log(`${c.id.padEnd(14)} ${archiveName}  ${(size / 1e6).toFixed(1)} MB  ${digest.slice(0, 16)}…`);
}

writeFileSync(join(OUT, "components.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`\nWrote ${join(OUT, "components.json")} — upload it LAST (after the .tar.gz files).`);
