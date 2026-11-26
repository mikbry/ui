// Zero-dependency screenshot workflow for the #278 shadcn/ui parity fixture.
// Serves this directory over plain HTTP (so the wasm module and ES module
// imports work — `file://` cannot), then drives headless Chrome the same
// way examples/web-recipe-smoke/check-cascade.mjs does (`--screenshot`
// instead of `--dump-dom`) to capture mkui.html and shadcn.html at every
// {light, dark} x {1280, 500} combination into screenshots/.
//
// Run `./build.sh` first (wasm pkg/ + compiled dist/shadcn.css). Usage:
//   node capture.mjs
import { createServer } from 'node:http';
import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';

// `spawnSync` would block this process's event loop for the subprocess's
// whole lifetime — including the in-process HTTP server below, which the
// very same subprocess needs to answer, deadlocking until the timeout.
// Run Chrome asynchronously instead so the server keeps serving requests
// while we await it.
function run(command, args, { timeout }) {
  return new Promise((done) => {
    const child = spawn(command, args, { timeout });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('close', (status) => done({ status, stdout, stderr }));
  });
}

const here = resolve(fileURLToPath(import.meta.url), '..');
const outDir = join(here, 'screenshots');

const MIME = {
  '.html': 'text/html', '.css': 'text/css', '.js': 'text/javascript',
  '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json',
};

for (const required of ['pkg/shadcn_parity.js', 'pkg/shadcn_parity_bg.wasm', 'dist/shadcn.css']) {
  if (!existsSync(join(here, required))) {
    throw new Error(`Missing ${required} — run ./build.sh first.`);
  }
}

function chromeBinary() {
  if (process.env.CHROME_BIN) return process.env.CHROME_BIN;
  return process.platform === 'darwin'
    ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
    : 'google-chrome';
}

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://localhost');
    let path = decodeURIComponent(url.pathname);
    if (path === '/') path = '/index.html';
    const file = join(here, path);
    if (!file.startsWith(here)) { res.writeHead(403); res.end(); return; }
    const body = await readFile(file);
    res.writeHead(200, { 'Content-Type': MIME[extname(file)] ?? 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end('not found');
  }
});

await new Promise((done) => server.listen(0, '127.0.0.1', done));
const port = server.address().port;

try {
  const chrome = chromeBinary();
  const chromeVersion = execFileSync(chrome, ['--version'], { encoding: 'utf8' }).trim();
  await mkdir(outDir, { recursive: true });

  // 500px, not the 375px a phone would use: this Chrome build clamps
  // `--window-size` to a 500px-wide minimum viewport in headless capture
  // (verified empirically — innerWidth reports 500 for every requested
  // width <= 500). True mobile-width emulation needs the DevTools
  // Protocol's Emulation.setDeviceMetricsOverride; deferred as a follow-up,
  // see README.md.
  const viewports = [[1280, 'desktop'], [500, 'narrow']];
  const themes = ['light', 'dark'];
  const pages = ['mkui', 'shadcn'];
  const manifest = {
    generatedAt: new Date().toISOString(),
    chrome: chromeVersion,
    platform: `${process.platform} ${process.arch}`,
    shadcnRefSha: '0e5b2e621a9ee4cf6a604bbe6c5c0d5b70561eb4',
    deviceScaleFactor: 1,
    shots: [],
  };

  for (const page of pages) {
    for (const theme of themes) {
      for (const [width, label] of viewports) {
        const name = `${page}-${theme}-${label}.png`;
        const userDataDir = mkdtempSync(join(tmpdir(), 'mkui-parity-'));
        const result = await run(chrome, [
          '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
          `--user-data-dir=${userDataDir}`, `--window-size=${width},900`,
          '--force-device-scale-factor=1', '--hide-scrollbars',
          `--screenshot=${join(outDir, name)}`, '--virtual-time-budget=4000',
          `http://127.0.0.1:${port}/${page}.html?theme=${theme}`,
        ], { timeout: 30000 });
        rmSync(userDataDir, { recursive: true, force: true });
        if (result.status !== 0) {
          throw new Error(`${name}: chrome exited ${result.status}\n${result.stderr}`);
        }
        // The in-page self-check (see mkui.html/shadcn.html) sets this once
        // its assertions pass; confirm it via --dump-dom rather than trust
        // the screenshot alone.
        const domUserDataDir = mkdtempSync(join(tmpdir(), 'mkui-parity-'));
        const dom = (await run(chrome, [
          '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
          `--user-data-dir=${domUserDataDir}`,
          '--dump-dom', '--virtual-time-budget=4000',
          `http://127.0.0.1:${port}/${page}.html?theme=${theme}`,
        ], { timeout: 30000 })).stdout;
        rmSync(domUserDataDir, { recursive: true, force: true });
        if (!dom.includes('data-smoke="pass"')) {
          throw new Error(`${name}: in-page self-check did not report data-smoke="pass"`);
        }
        manifest.shots.push({ name, page, theme, width, label });
        console.log(`captured ${name}`);
      }
    }
  }

  await writeFile(join(outDir, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`\nWrote ${manifest.shots.length} screenshots + manifest.json to ${outDir}`);
} finally {
  server.close();
}
