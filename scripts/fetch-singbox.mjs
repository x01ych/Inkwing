#!/usr/bin/env node
import { createWriteStream, existsSync, readFileSync, statSync } from 'node:fs';
import { chmod, copyFile, mkdir, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';

const SING_BOX_VERSION = '1.13.11';
const WINTUN_VERSION = '0.14.1';

const here = dirname(fileURLToPath(import.meta.url));
const projectRoot = resolve(here, '..');
const binariesDir = join(projectRoot, 'src-tauri', 'binaries');
const cacheDir = join(tmpdir(), 'inkwing-fetch-cache');
const isWindows = process.platform === 'win32';

// All known sing-box sidecar targets. Tauri 2 picks the one matching the
// host triple at build/dev time, so it's safe to keep them all here even
// on a single-platform dev box (extra ~80 MiB cached). Use --skip-foreign
// to download only the host's target (handy for slow links / CI).
const TARGETS = [
  {
    label: 'linux x86_64',
    archive: `sing-box-${SING_BOX_VERSION}-linux-amd64.tar.gz`,
    extractedBin: 'sing-box',
    sidecarName: 'sing-box-x86_64-unknown-linux-gnu',
    isZip: false,
    triple: 'x86_64-unknown-linux-gnu',
  },
  {
    label: 'windows x86_64',
    archive: `sing-box-${SING_BOX_VERSION}-windows-amd64.zip`,
    extractedBin: 'sing-box.exe',
    sidecarName: 'sing-box-x86_64-pc-windows-msvc.exe',
    isZip: true,
    triple: 'x86_64-pc-windows-msvc',
  },
  {
    label: 'macos x86_64 (Intel)',
    archive: `sing-box-${SING_BOX_VERSION}-darwin-amd64.tar.gz`,
    extractedBin: 'sing-box',
    sidecarName: 'sing-box-x86_64-apple-darwin',
    isZip: false,
    triple: 'x86_64-apple-darwin',
  },
  {
    label: 'macos arm64 (Apple Silicon)',
    archive: `sing-box-${SING_BOX_VERSION}-darwin-arm64.tar.gz`,
    extractedBin: 'sing-box',
    sidecarName: 'sing-box-aarch64-apple-darwin',
    isZip: false,
    triple: 'aarch64-apple-darwin',
  },
];

/** Detect the host's Tauri target triple, used by `--skip-foreign`. */
function hostTriple() {
  const { platform, arch } = process;
  if (platform === 'linux' && arch === 'x64') return 'x86_64-unknown-linux-gnu';
  if (platform === 'win32' && arch === 'x64') return 'x86_64-pc-windows-msvc';
  if (platform === 'darwin' && arch === 'x64') return 'x86_64-apple-darwin';
  if (platform === 'darwin' && arch === 'arm64') return 'aarch64-apple-darwin';
  return null;
}

const WINTUN = {
  url: `https://www.wintun.net/builds/wintun-${WINTUN_VERSION}.zip`,
  innerPath: 'wintun/bin/amd64/wintun.dll',
  outName: 'wintun-amd64.dll',
};

function log(...args) {
  console.log('[fetch-singbox]', ...args);
}

async function ensureDir(p) {
  if (!existsSync(p)) await mkdir(p, { recursive: true });
}

async function download(url, dest) {
  // Atomic write: stream to <dest>.partial, then rename. A killed
  // process / network drop leaves the .partial behind (which we
  // happily overwrite next run) instead of poisoning the cache with
  // a truncated archive masquerading as a complete one.
  log('GET', url);
  const tmp = `${dest}.partial`;
  // Best-effort cleanup of any leftover .partial from a prior failure.
  if (existsSync(tmp)) await rm(tmp, { force: true });
  const res = await fetch(url, { redirect: 'follow' });
  if (!res.ok) throw new Error(`HTTP ${res.status} for ${url}`);
  await pipeline(Readable.fromWeb(res.body), createWriteStream(tmp));
  await rename(tmp, dest);
  const size = statSync(dest).size;
  log(`  -> ${dest} (${(size / 1024 / 1024).toFixed(2)} MiB)`);
  return dest;
}

/** Best-effort integrity probe — list the archive without extracting.
 * Returns true if listing succeeds, false otherwise. Callers use this
 * to detect cache files that were left truncated by an old run of the
 * script (before atomic download was added). */
async function verifyArchive(archivePath, isZip) {
  const candidates = isZip
    ? isWindows
      ? [
          { cmd: 'tar', args: ['-tf', archivePath] },
          {
            cmd: 'powershell.exe',
            args: [
              '-NoProfile',
              '-ExecutionPolicy',
              'Bypass',
              '-Command',
              '& { param($p) [System.IO.Compression.ZipFile]::OpenRead($p).Dispose() }',
              archivePath,
            ],
          },
        ]
      : [{ cmd: 'unzip', args: ['-tq', archivePath] }]
    : [{ cmd: 'tar', args: ['-tzf', archivePath] }];
  for (const c of candidates) {
    try {
      await run(c.cmd, c.args, { stdio: 'ignore' });
      return true;
    } catch {
      // try the next candidate
    }
  }
  return false;
}

function sha256(path) {
  const h = createHash('sha256');
  h.update(readFileSync(path));
  return h.digest('hex');
}

function run(cmd, args, opts = {}) {
  return new Promise((resolveP, rejectP) => {
    const p = spawn(cmd, args, { stdio: 'inherit', ...opts });
    p.on('error', rejectP);
    p.on('exit', (code) =>
      code === 0 ? resolveP() : rejectP(new Error(`${cmd} ${args.join(' ')} exited ${code}`))
    );
  });
}

async function extractArchive(archivePath, isZip, outDir) {
  await ensureDir(outDir);
  if (isZip) {
    await extractZip(archivePath, outDir);
  } else {
    await run('tar', ['-xzf', archivePath, '-C', outDir]);
  }
}

async function extractZip(archivePath, outDir) {
  const extractors = isWindows
    ? [
        {
          label: 'tar.exe',
          cmd: 'tar',
          args: ['-xf', archivePath, '-C', outDir],
        },
        {
          label: 'PowerShell Expand-Archive',
          cmd: 'powershell.exe',
          args: [
            '-NoProfile',
            '-ExecutionPolicy',
            'Bypass',
            '-Command',
            '& { param($archive, $dest) Expand-Archive -LiteralPath $archive -DestinationPath $dest -Force }',
            archivePath,
            outDir,
          ],
        },
      ]
    : [
        {
          label: 'unzip',
          cmd: 'unzip',
          args: ['-q', '-o', archivePath, '-d', outDir],
        },
      ];

  let lastError = null;
  for (const extractor of extractors) {
    try {
      await run(extractor.cmd, extractor.args);
      return;
    } catch (e) {
      lastError = e;
      log(`WARN: ${extractor.label} failed (${e.message})`);
    }
  }

  throw lastError ?? new Error(`could not extract ${archivePath}`);
}

async function makeExecutable(path) {
  if (isWindows) return;
  await chmod(path, 0o755);
}

async function findFile(root, predicate) {
  const entries = await readdir(root, { withFileTypes: true });
  for (const e of entries) {
    const full = join(root, e.name);
    if (e.isDirectory()) {
      const hit = await findFile(full, predicate);
      if (hit) return hit;
    } else if (predicate(e.name, full)) {
      return full;
    }
  }
  return null;
}

async function fetchSingBox(filter) {
  await ensureDir(binariesDir);
  await ensureDir(cacheDir);

  const targets = filter ? TARGETS.filter(filter) : TARGETS;
  for (const t of targets) {
    const url = `https://github.com/SagerNet/sing-box/releases/download/v${SING_BOX_VERSION}/${t.archive}`;
    const archivePath = join(cacheDir, t.archive);
    if (existsSync(archivePath)) {
      log(`cache hit: ${t.archive}`);
      const ok = await verifyArchive(archivePath, t.isZip);
      if (!ok) {
        log(`  cache corrupt (failed integrity probe) — re-downloading`);
        await rm(archivePath, { force: true });
        await download(url, archivePath);
      }
    } else {
      await download(url, archivePath);
    }
    const digest = sha256(archivePath);
    log(`  sha256: ${digest}`);

    const extractDir = join(cacheDir, `extract-${t.archive}`);
    await rm(extractDir, { recursive: true, force: true });
    await extractArchive(archivePath, t.isZip, extractDir);

    const found = await findFile(extractDir, (name) => name === t.extractedBin);
    if (!found) throw new Error(`could not find ${t.extractedBin} inside ${t.archive}`);

    const dest = join(binariesDir, t.sidecarName);
    await copyFile(found, dest);
    if (!t.isZip) await makeExecutable(dest);
    log(`  installed -> ${dest}`);
  }
}

async function fetchWintun() {
  await ensureDir(cacheDir);
  const archivePath = join(cacheDir, `wintun-${WINTUN_VERSION}.zip`);
  if (existsSync(archivePath)) {
    log(`cache hit: wintun-${WINTUN_VERSION}.zip`);
    const ok = await verifyArchive(archivePath, true);
    if (!ok) {
      log(`  cache corrupt (failed integrity probe) — re-downloading`);
      await rm(archivePath, { force: true });
      await download(WINTUN.url, archivePath);
    }
  } else {
    await download(WINTUN.url, archivePath);
  }
  const digest = sha256(archivePath);
  log(`  sha256: ${digest}`);

  const extractDir = join(cacheDir, `extract-wintun-${WINTUN_VERSION}`);
  await rm(extractDir, { recursive: true, force: true });
  await extractArchive(archivePath, true, extractDir);

  const dllPath = join(extractDir, WINTUN.innerPath);
  if (!existsSync(dllPath)) {
    const found = await findFile(extractDir, (name) => name.toLowerCase() === 'wintun.dll');
    if (!found) throw new Error(`wintun.dll not found in archive`);
    await copyFile(found, join(binariesDir, WINTUN.outName));
  } else {
    await copyFile(dllPath, join(binariesDir, WINTUN.outName));
  }
  log(`  installed -> ${join(binariesDir, WINTUN.outName)}`);
}

async function writeStamp() {
  const stamp = {
    sing_box_version: SING_BOX_VERSION,
    wintun_version: WINTUN_VERSION,
    fetched_at: new Date().toISOString(),
  };
  await writeFile(join(binariesDir, 'VERSION.json'), JSON.stringify(stamp, null, 2));
}

async function main() {
  const skipWintun = process.argv.includes('--no-wintun');
  const skipForeign = process.argv.includes('--skip-foreign');
  let filter = null;
  if (skipForeign) {
    const triple = hostTriple();
    if (!triple) {
      throw new Error(`unrecognised host (${process.platform}/${process.arch}); cannot --skip-foreign`);
    }
    filter = (t) => t.triple === triple;
    log(`--skip-foreign: only fetching host target ${triple}`);
  }
  log(`sing-box ${SING_BOX_VERSION}, wintun ${WINTUN_VERSION}`);
  log(`output: ${binariesDir}`);
  await fetchSingBox(filter);
  // wintun is Windows-only; skip when host is anything else AND we're
  // restricting to host-only.
  const wantWintun = !skipWintun && (!skipForeign || hostTriple() === 'x86_64-pc-windows-msvc');
  if (wantWintun) {
    try {
      await fetchWintun();
    } catch (e) {
      log(`WARN: wintun fetch failed (${e.message}); continuing — non-Windows hosts do not need it`);
    }
  } else if (skipForeign) {
    log('skipping wintun (--skip-foreign on non-Windows host)');
  }
  // Tauri's build script verifies every entry in bundle.resources up
  // front and fails if a path is missing — even on platforms that won't
  // ever load that resource. We always need *some* file at
  // binaries/wintun-amd64.dll so the glob matches; if the real DLL
  // wasn't downloaded (Linux/macOS dev), drop a 0-byte placeholder.
  const wintunPath = join(binariesDir, WINTUN.outName);
  if (!existsSync(wintunPath)) {
    await writeFile(wintunPath, Buffer.alloc(0));
    log(`created empty placeholder ${WINTUN.outName} (Tauri bundle.resources requires it; non-Windows hosts will never load it)`);
  }
  await writeStamp();
  log('done.');
}

main().catch((err) => {
  console.error('[fetch-singbox] FAILED:', err.message);
  process.exit(1);
});
