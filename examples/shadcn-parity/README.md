# shadcn/ui parity fixture (#278)

A runnable, side-by-side comparison of the production `mkui-web` DOM
renderer against a pinned public shadcn/ui reference, starting with `Card`
and `Button`. No WebGPU: this is a pure DOM/CSS fixture, buildable and
screenshottable independently of the wgpu backend.

- `index.html` — the human-facing side-by-side viewer (two `<iframe>`s,
  theme + viewport toggles). Open this to look at the comparison live.
- `mkui.html` — the `mkui-web` pane. Mounts `src/lib.rs`'s fixture through
  `mkui_web::Mkui`.
- `shadcn.html` — the shadcn/ui reference pane. Static markup using the
  pinned component source's own class strings and `data-slot` attributes,
  compiled with the project's locked Tailwind 4.
- `capture.mjs` — the screenshot workflow (below).

## Pinned reference

Component source and design tokens are pinned to a single commit so the
comparison doesn't silently drift as shadcn/ui's registry changes:

- Commit: `shadcn-ui/ui@0e5b2e621a9ee4cf6a604bbe6c5c0d5b70561eb4`
- Style: `new-york-v4`, base color: `neutral`
- `Button`: `apps/v4/registry/new-york-v4/ui/button.tsx`
- `Card`: `apps/v4/registry/new-york-v4/ui/card.tsx`
- Design tokens: `apps/v4/registry/themes.ts` (the `"neutral"` theme's
  `cssVars`) + the `@theme inline` radius/color mapping from
  `apps/v4/app/globals.css`, with the docs-site-only utilities (code
  blocks, `steps`, etc.) stripped — see `tokens.css` and
  `shadcn-source.css`.

Both `button.tsx`'s and `card.tsx`'s exact Tailwind class strings and
`data-slot`/`data-variant`/`data-size` attributes are reproduced verbatim
in `shadcn.html`; nothing here was written from memory of what shadcn/ui
"usually" looks like.

## Build

```sh
npm ci --prefix examples/web-showcase   # once, for the locked Tailwind 4 CLI
examples/shadcn-parity/build.sh
```

This builds the `mkui-web` wasm module (`pkg/`, via `wasm-pack`) and
compiles the shadcn reference pane's CSS (`dist/shadcn.css`, via the
Tailwind 4 CLI already locked in `examples/web-showcase/node_modules` —
`shadcn-source.css`'s `@import "tailwindcss"` resolves against a
`node_modules` symlink `build.sh` creates and never commits; see
`.gitignore`). Set `TAILWIND_NODE_MODULES` to use a different install.

## Run

```sh
python3 -m http.server 8000 --directory examples/shadcn-parity
```

Open `http://localhost:8000/index.html`. Toggle theme (light/dark) and
viewport (desktop/narrow) above the two panes; both update together.

## Screenshot workflow

```sh
node examples/shadcn-parity/capture.mjs
```

Zero additional npm dependencies: it serves this directory with a small
`node:http` server, drives headless Chrome directly (the same
`--screenshot`/`--dump-dom` pattern as
`examples/web-recipe-smoke/check-cascade.mjs`), and writes 8 PNGs plus a
`manifest.json` into `screenshots/`:

- `{mkui,shadcn}-{light,dark}-{desktop,narrow}.png`

`desktop` is 1280px wide. `narrow` is 500px, not the 375px a phone would
use — this Chrome build clamps `--window-size`'s headless viewport to a
500px-wide minimum (verified empirically: `window.innerWidth` reports 500
for every requested width ≤ 500). True mobile-width emulation needs the
DevTools Protocol's `Emulation.setDeviceMetricsOverride`; deferred (see
"Deferred" below), since 500px already exercises the responsive layout
meaningfully against the 1280px desktop baseline.

Each capture is verified two ways before it's accepted: the screenshot
itself, and a separate `--dump-dom` pass asserting the page's own
in-browser self-check (embedded in `mkui.html`/`shadcn.html`, same
`data-smoke` pattern as `web-recipe-smoke`) reports `data-smoke="pass"`.
That self-check asserts, on both panes: the focus target and the disabled
button are both plain `button[type="button"]`; the disabled button carries
the native `disabled` attribute and 50% opacity; and the card's border is
present. Re-run `capture.mjs` any time the fixture or CSS changes — the
committed PNGs in `screenshots/` are this example's regression baseline,
reviewable across runs via git's own image diff, on this same machine/
Chrome version (recorded in `screenshots/manifest.json`); a different
machine or Chrome version can shift anti-aliasing and should be judged by
re-running the self-check, not by a raw pixel diff against the baseline.

`capture.mjs` always focuses the fixture's fourth button
(`.parity-btn-focus-target` / `#focus-target`) via `.focus()` before
capturing, so every screenshot already shows the `:focus-visible` ring —
there's no separate "focused" axis in the matrix. A real, trusted `Tab`
keypress is a manual check (open `index.html`, click into a pane, press
`Tab`); `.focus()` is this fixture's reproducible stand-in for CI/headless
use, consistent with how `.click()` stands in for a trusted click in
`web-recipe-smoke`.

## Host-side contract test

`src/lib.rs`'s `#[cfg(test)]` module asserts the fixture's shape against
the shared `mkui-core` `AppTree` directly — one `Card`, six `Button`s,
exactly one disabled, and the expected label/variant/disabled combinations
— with no browser, as part of `cargo test --workspace`.

## Behavior and accessibility comparison

| Check | mkui-web | shadcn/ui (pinned) |
|---|---|---|
| Button element | `<button type="button">` | `<button data-slot="button" data-variant="…" data-size="…" type="button">` (same native semantics; shadcn additionally exposes variant/size as data attributes for styling hooks, mkui-web does not) |
| Disabled semantics | Native `disabled` attribute. `opacity: 0.5`. | Native `disabled` attribute. `opacity: 0.5`. |
| Disabled + overlap (**intentional difference**, see `crates/web/styles.css`) | `pointer-events: auto` — the painted (dimmed) button still blocks clicks to anything beneath it. | `disabled:pointer-events-none` — clicks pass through to whatever is underneath. |
| Keyboard focus | `:focus-visible` → `border-color` + `0 0 0 3px` ring in `--ring` at 50% opacity (hand-matched in `mkui.css`). | `focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50` (same visual recipe, Tailwind-generated). |
| Tab order | Native; no `tabindex` set. | Native; no `tabindex` set. |
| Card DOM structure (**known gap**, tracked in #276) | One flat `<div class="card">`; header/description/content/footer are plain nested `<div>`/`<p>` this fixture's own classes target — no dedicated sub-component API. | `data-slot="card"` plus distinct `card-header`/`card-title`/`card-description`/`card-content`/`card-footer` slot elements, each independently stylable. |
| Card corner radius | Reads `--radius` directly (`crates/web/styles.css`'s `DEFAULT_COMPONENT_CSS`): `0.625rem`. | Tailwind's `rounded-xl`, mapped to `--radius-xl: calc(var(--radius) * 1.4)` = `0.875rem`. |
| Button chrome CSS | `mkui-web` ships none (`DEFAULT_COMPONENT_CSS` only covers `Card` + disabled opacity, see `crates/web/styles.css`) — this fixture's `mkui.css` is a hand-matched approximation of the pinned recipe below, not generated from it. | Generated by Tailwind 4 from `button.tsx`'s `cva` string, compiled via `shadcn-source.css`. |
| Button dark-mode outline fill | Flat `background-color: var(--background)` in both themes (no dark-specific rule in `mkui.css`). | `dark:bg-input/30` gives the outline variant a faint fill in dark mode that `light` mode doesn't have. |
| Theming mechanism | `mkui_web::WebApp` applies its own `dark` class to `<body>` from `ThemeMode::System`/`prefers-color-scheme` *during* wasm init, before this fixture's page script runs — `mkui.html`'s `setTheme()` re-applies the requested `?theme=`/`postMessage` value to `<body>` *after* `init()` resolves so it wins the cascade. See the comment in `mkui.html`. | Plain `.dark` class on `<html>`, toggled directly by this fixture's own script; no competing framework-level logic. |

Record further intentional differences you notice here as you extend this
fixture — this table, not a follow-up issue, is the source of truth for
"the two sides disagree here on purpose."

## Deferred

Out of scope for #278's acceptance criteria, filed as follow-up candidates
rather than blocking this PR:

- Automated pixel diffing between `screenshots/` runs (today: self-check
  assertions + manual visual review).
- Wiring `capture.mjs` into CI (today: a local, documented, reproducible
  command).
- True mobile-width (375px) viewport emulation via the DevTools Protocol,
  instead of headless Chrome's 500px `--window-size` floor.
