// No test dependencies: use the showcase's installed Tailwind CLI and Chrome.
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, copyFileSync, symlinkSync, rmSync, existsSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { spawnSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../..');
const modules = resolve(process.env.TAILWIND_NODE_MODULES || join(root, 'examples/web-showcase/node_modules'));
const cli = join(modules, '@tailwindcss/cli/dist/index.mjs');
if (!existsSync(cli)) throw new Error('Install examples/web-showcase dependencies with npm ci, or set TAILWIND_NODE_MODULES.');
const chrome = process.env.CHROME_BIN || (process.platform === 'darwin'
  ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : 'google-chrome');
const temp = mkdtempSync(join(tmpdir(), 'mkui-cascade-'));
const fixture = join(temp, 'examples/web-recipe-smoke');
mkdirSync(fixture, { recursive: true });
mkdirSync(join(temp, 'crates/web'), { recursive: true });
const showcase = join(temp, 'examples/web-showcase');
mkdirSync(showcase, { recursive: true });
for (const file of readdirSync(join(root, 'examples/web-showcase')).filter(file => file.endsWith('.css'))) {
  copyFileSync(join(root, 'examples/web-showcase', file), join(showcase, file));
}
// Absolute symlink exists only in the temporary fixture, never in the repository.
symlinkSync(modules, join(temp, 'node_modules'), 'dir');
copyFileSync(join(root, 'crates/web/styles.css'), join(temp, 'crates/web/styles.css'));
copyFileSync(join(here, 'cascade.html'), join(fixture, 'cascade.html'));
const input = readFileSync(join(here, 'tailwind.css'), 'utf8');
const html = readFileSync(join(here, 'cascade.html'), 'utf8');

function run(command, args) {
  const result = spawnSync(command, args, { cwd: temp, encoding: 'utf8', timeout: 60000, maxBuffer: 8 * 1024 * 1024 });
  if (result.error || result.status !== 0) {
    throw new Error(`${command} failed: ${result.error || result.signal || result.status}\n${result.stderr}`);
  }
  return result.stdout;
}

try {
  for (const mode of ['tailwind', 'showcase', 'plain', 'regressed']) {
    let css = '../../crates/web/styles.css';
    if (mode !== 'plain') {
      // Negative control reproduces the shipped ordering, without the prelude.
      let source = mode === 'regressed' ? input
        .replace('@layer theme, base, mkui-defaults, components, utilities;', '')
        .replace('@import "tailwindcss" source(none);\n@import "../../crates/web/styles.css";',
          '@import "../../crates/web/styles.css";\n@import "tailwindcss" source(none);') : input;
      if (mode === 'showcase') {
        source = '@import "../web-showcase/styles.css";\n@source "./cascade.html";\n' +
          input.slice(input.indexOf('@layer components'));
      }
      writeFileSync(join(fixture, `${mode}.css`), source);
      css = `${mode}.compiled.css`;
      run(process.execPath, [cli, '-i', join(fixture, `${mode}.css`), '-o', join(fixture, css), '--minify']);
    }
    let page = html.replace('__CSS__', css).replace('__TAILWIND__', String(mode !== 'plain'));
    if (mode === 'showcase') {
      // Normalize application-owned tokens so the same assertions test precedence.
      page = page.replace('<html lang="en">', '<html lang="en" style="--card: #fff; --card-foreground: #18181b; --border: #e4e4e7; --radius: 0.5rem">');
    }
    writeFileSync(join(fixture, `${mode}.html`), page);
  }
  console.log(`Compiled fixture: ${fixture}`);
  if (!process.argv.includes('--build-only')) {
    for (const mode of ['tailwind', 'showcase', 'plain', 'regressed']) {
      const dom = run(chrome, ['--headless', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
        `--user-data-dir=${join(temp, 'chrome-profile')}`, '--dump-dom', '--virtual-time-budget=10000',
        pathToFileURL(join(fixture, `${mode}.html`)).href]);
      const expected = mode === 'regressed' ? 'fail' : 'pass';
      if (!dom.includes(`data-smoke="${expected}"`) ||
          (mode === 'regressed' && !dom.includes('FAIL: Error: Card border survives base reset'))) {
        throw new Error(`${mode}: unexpected browser result\n${dom}`);
      }
      console.log(`PASS: ${mode}${mode === 'regressed' ? ' (old ordering rejected)' : ''}`);
    }
    rmSync(temp, { recursive: true, force: true });
  }
} catch (error) {
  console.error(`Fixture retained for manual browser checks: ${fixture}`);
  throw error;
}
