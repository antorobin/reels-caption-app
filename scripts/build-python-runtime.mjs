// Builds the bundled, relocatable Python environments that replace the
// per-machine conda setup -- one self-contained interpreter + pinned
// wheels per feature, under src-tauri/resources/python/<env>/.
//
//   node scripts/build-python-runtime.mjs --pack=core     # stt only (goes in the MSI)
//   node scripts/build-python-runtime.mjs --pack=extras   # tts + media-ai + voice-clone
//   node scripts/build-python-runtime.mjs --pack=all
//   node scripts/build-python-runtime.mjs --env=stt       # one env by name
//
// Why python-build-standalone (Astral) and not conda-pack: these builds
// are *made* to be relocated -- no `conda-unpack` path-rewriting step, no
// activation scripts, `python.exe` works wherever the tree lands. That is
// exactly what python_env.rs expects: it runs `<resources>/python/<env>/
// python.exe <script>` directly, never `conda run`.
//
// Requires: `tar` (bundled with Windows 10+, macOS, Linux) and network
// access. Everything else -- the interpreter, pip -- is downloaded here.
//
// After running this, add the matching entry to tauri.conf.json's
// `bundle.resources` (see docs/RUNTIME-PACKS.md) and rebuild the MSI.

import { execFileSync } from "node:child_process";
import { createWriteStream, existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";

const HERE = dirname(fileURLToPath(import.meta.url));
const SRC_TAURI = resolve(HERE, "..", "src-tauri");
const REQ_DIR = join(SRC_TAURI, "requirements");
const OUT_DIR = join(SRC_TAURI, "resources", "python");

// python-build-standalone release. `install_only` is the relocatable
// variant. Bump the tag/version together; keep 3.11 -- mediapipe 0.10.9
// (media-ai) has no wheel for 3.12+.
const PBS_TAG = "20250106";
const PY_VERSION = "3.11.11";
const PLATFORMS = {
  win32: "x86_64-pc-windows-msvc",
  darwin: process.arch === "arm64" ? "aarch64-apple-darwin" : "x86_64-apple-darwin",
  linux: "x86_64-unknown-linux-gnu",
};

// Which requirements file feeds which env, and whether the OpenVoice git
// package needs the --no-deps dance (see requirements/voice-clone.txt).
const ENVS = {
  stt: { req: "stt.txt", pack: "core" },
  tts: { req: "tts.txt", pack: "extras" },
  "media-ai": { req: "media-ai.txt", pack: "extras" },
  "voice-clone": { req: "voice-clone.txt", pack: "extras", noDepsGit: true },
};

function parseArgs() {
  const args = Object.fromEntries(
    process.argv.slice(2).map((a) => {
      const [k, v] = a.replace(/^--/, "").split("=");
      return [k, v ?? true];
    }),
  );
  if (args.env) return [args.env];
  const pack = args.pack || "all";
  return Object.entries(ENVS)
    .filter(([, cfg]) => pack === "all" || cfg.pack === pack)
    .map(([name]) => name);
}

function pbsUrl() {
  const triple = PLATFORMS[process.platform];
  if (!triple) throw new Error(`Unsupported platform: ${process.platform}`);
  return `https://github.com/astral-sh/python-build-standalone/releases/download/${PBS_TAG}/cpython-${PY_VERSION}+${PBS_TAG}-${triple}-install_only.tar.gz`;
}

async function download(url, dest) {
  process.stdout.write(`  fetching ${url.split("/").pop()} … `);
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);
  await pipeline(Readable.fromWeb(res.body), createWriteStream(dest));
  console.log("done");
}

function run(cmd, args, opts = {}) {
  execFileSync(cmd, args, { stdio: "inherit", ...opts });
}

function pythonExe(prefix) {
  return process.platform === "win32" ? join(prefix, "python.exe") : join(prefix, "bin", "python3");
}

async function buildEnv(name) {
  const cfg = ENVS[name];
  if (!cfg) throw new Error(`Unknown env "${name}". Known: ${Object.keys(ENVS).join(", ")}`);
  const reqPath = join(REQ_DIR, cfg.req);
  if (!existsSync(reqPath)) throw new Error(`Missing ${reqPath}`);

  const prefix = join(OUT_DIR, name);
  console.log(`\n=== ${name}  ->  ${prefix}`);
  rmSync(prefix, { recursive: true, force: true });
  mkdirSync(prefix, { recursive: true });

  // 1. relocatable interpreter
  const tarball = join(OUT_DIR, `_pbs-${name}.tar.gz`);
  await download(pbsUrl(), tarball);
  // install_only tarballs unpack to a top-level `python/` dir
  run("tar", ["-xzf", tarball, "-C", prefix, "--strip-components=1"]);
  rmSync(tarball, { force: true });

  const py = pythonExe(prefix);

  // 2. wheels. --only-binary=:all: is the whole point: if something needs
  //    a compiler, the pin is wrong -- fix the version, don't reach for
  //    conda.
  run(py, ["-m", "pip", "install", "--upgrade", "pip"]);

  if (cfg.noDepsGit) {
    // OpenVoice: its own pinned deps don't have modern wheels. Install the
    // git package alone, then its real deps from the same requirements
    // file (which lists both, the git line and the explicit deps).
    const req = await readFile(reqPath, "utf8");
    const gitLine = req.split("\n").find((l) => l.trim().startsWith("openvoice") && l.includes("git+"));
    const rest = req
      .split("\n")
      .filter((l) => l.trim() && !l.trim().startsWith("#") && !l.includes("git+"))
      .join("\n");
    const restPath = join(OUT_DIR, `_req-${name}.txt`);
    const { writeFile } = await import("node:fs/promises");
    await writeFile(restPath, rest);
    run(py, ["-m", "pip", "install", "--only-binary=:all:", "-r", restPath]);
    if (gitLine) run(py, ["-m", "pip", "install", "--no-deps", gitLine.trim()]);
    rmSync(restPath, { force: true });
  } else {
    run(py, ["-m", "pip", "install", "--only-binary=:all:", "-r", reqPath]);
  }

  // 3. shrink: nothing at runtime needs pip's cache, bytecode caches,
  //    packages' bundled test suites, or the pip/setuptools trees
  //    themselves (we never install at runtime). Skippable with --no-prune.
  run(py, ["-m", "pip", "cache", "purge"], { stdio: "ignore" });
  if (process.argv.includes("--no-prune")) {
    console.log(`  ${name} built (unpruned).`);
    return;
  }
  const before = dirSizeMB(prefix);
  pruneEnv(prefix);
  console.log(`  ${name} built + pruned: ${before} MB -> ${dirSizeMB(prefix)} MB`);
}

function pruneEnv(prefix) {
  const sp = join(prefix, "Lib", "site-packages");
  for (const junk of ["pip", "setuptools", "pkg_resources", "_distutils_hack"]) {
    rmSync(join(sp, junk), { recursive: true, force: true });
  }
  // Left dangling once _distutils_hack is gone; prints a harmless
  // ModuleNotFoundError to stderr on every interpreter start otherwise.
  rmSync(join(sp, "distutils-precedence.pth"), { force: true });
  rmSync(join(prefix, "Scripts"), { recursive: true, force: true });
  walkRemove(prefix, (name, isDir) => isDir && (name === "__pycache__" || name === "tests" || name === "test"));
}

function walkRemove(dir, matchFn) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (matchFn(e.name, e.isDirectory())) {
      rmSync(p, { recursive: true, force: true });
    } else if (e.isDirectory()) {
      walkRemove(p, matchFn);
    }
  }
}

function dirSizeMB(dir) {
  let total = 0;
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return 0;
  }
  for (const e of entries) {
    const p = join(dir, e.name);
    if (e.isDirectory()) total += dirSizeMB(p) * 1e6;
    else {
      try {
        total += statSync(p).size;
      } catch {
        /* gone */
      }
    }
  }
  return Math.round(total / 1e6);
}

const targets = parseArgs();
console.log(`Building: ${targets.join(", ")}`);
mkdirSync(OUT_DIR, { recursive: true });
for (const name of targets) {
  await buildEnv(name);
}
console.log("\nAll requested envs built under src-tauri/resources/python/.");
console.log("Next: prune, then add to tauri.conf.json bundle.resources (docs/RUNTIME-PACKS.md).");
