// Downloads every large, static binary/model this app needs (ffmpeg,
// llama.cpp + Qwen2.5-0.5B-Instruct, Piper's English voices, the Tamil
// Whisper checkpoint, the OpenVoice V2 converter checkpoint) in one shot
// from this repo's GitHub Release assets, and unpacks them into
// src-tauri/resources/.
//
// Why a release asset instead of Git LFS: these files never change once
// fetched, but LFS bills by storage *and* by bandwidth on every clone
// regardless of that -- a poor fit for something that's fetched once and
// then just sits there. A release asset has no comparable per-clone cost
// and a much higher (2GB) per-file limit, so a single archive works fine
// even though the whole bundle is ~1.5GB.
//
// Extraction uses the system `tar` (bundled with Windows 10+, macOS, and
// every mainstream Linux distro today) rather than adding an npm
// dependency just to unzip one archive.

import { createWriteStream, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const projectRoot = path.resolve(__dirname, "..");

const ASSET_NAME = "dev-resources.tar.gz";
const DEFAULT_REPO = "antorobin/reels-caption-app";

function resolveRepo() {
  const result = spawnSync("git", ["remote", "get-url", "origin"], { cwd: projectRoot, encoding: "utf-8" });
  const url = result.stdout?.trim();
  const match = url?.match(/github\.com[/:]([^/]+\/[^/.]+)(?:\.git)?$/);
  return match ? match[1] : DEFAULT_REPO;
}

// A passthrough Transform (not a plain Writable -- a Writable has no
// readable side to pipe onward, so `pipeline(source, progress, dest)`
// needs `progress` to actually be a Duplex/Transform for the data to
// reach `dest` at all) that just reports each chunk's size as it flows
// through, for a periodic progress log during the ~1.5GB download.
function progressReporter(total) {
  let received = 0;
  let lastPercent = -1;
  return new Transform({
    transform(chunk, _enc, callback) {
      received += chunk.length;
      if (total > 0) {
        const percent = Math.floor((received / total) * 100);
        if (percent !== lastPercent && percent % 5 === 0) {
          lastPercent = percent;
          process.stdout.write(`\r  downloading… ${percent}% (${(received / 1e6).toFixed(0)}MB / ${(total / 1e6).toFixed(0)}MB)`);
        }
      }
      callback(null, chunk);
    },
  });
}

async function downloadTo(url, destPath) {
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok) {
    throw new Error(`Download failed: ${response.status} ${response.statusText} (${url})`);
  }
  const total = Number(response.headers.get("content-length") ?? 0);

  await pipeline(Readable.fromWeb(response.body), progressReporter(total), createWriteStream(destPath));
  process.stdout.write("\n");
}

async function main() {
  const repo = resolveRepo();
  const url = `https://github.com/${repo}/releases/latest/download/${ASSET_NAME}`;
  console.log(`Fetching dev resources from ${url} …`);

  const tmpDir = mkdtempSync(path.join(tmpdir(), "reels-caption-app-resources-"));
  const archivePath = path.join(tmpDir, ASSET_NAME);

  try {
    await downloadTo(url, archivePath);

    console.log("Extracting into src-tauri/resources/ …");
    const result = spawnSync("tar", ["-xzf", archivePath, "-C", projectRoot], { stdio: "inherit" });
    if (result.status !== 0) {
      throw new Error(`tar extraction failed with exit code ${result.status}`);
    }

    console.log("Done. Bundled resources are in src-tauri/resources/.");
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error(`\nfetch-dev-resources failed: ${err.message}`);
  console.error("You can also fetch each piece manually — see the README's setup sections (2, 2.3, 2.6, 2.7).");
  process.exit(1);
});
