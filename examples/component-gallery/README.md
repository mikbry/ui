# mkui Component Gallery

A native (`mkui-wgpu`) catalogue of every component currently promoted into
the shared `mkui-core` contract — today, [`Button`](../../crates/core/src/components.rs)
and [`Card`](../../crates/core/src/components.rs). One section per
component, with its meaningful state variants shown side by side.

This is the native counterpart to #278's web/shadcn-parity fixture, but
with no external reference to compare against — its only job is to keep
every contract-level component visible and inspectable on the native
backend as #270's promotion programme adds more of them. It is
**additive**: `examples/native-showcase` / `examples/showcase-common`'s
existing marketing-style landing page is unchanged.

Tracked at [#276](https://github.com/mikbry/ui/issues/276) (the
shadcn/ui catalogue tracker) — add this gallery's sections next to a
row there as each component clears the three-backend bar.

## Launch

```bash
cargo run -p component-gallery --release
```

Opens a window (title `mkui`) rendering the gallery. Close the window, or
`Ctrl+C` the process, to exit.

### CI / headless smoke

```bash
HEADLESS=1 cargo run -p component-gallery --release
```

Exits after a single walker pass with no display server required — the
same `HEADLESS` contract `native-showcase` and the other wgpu examples
use. This does not produce a screenshot; it only proves the tree builds
and walks without panicking.

## Capturing a screenshot

There is no in-process screenshot capture in `mkui-wgpu` today (see ADR
0006, "Visual regression / screenshot diff infrastructure" — deferred).
Until that lands, capture is a manual, OS-native step:

1. Run the gallery with a fixed window size by leaving it at its default
   (do not resize the window) — this keeps captures comparable across
   runs: `cargo run -p component-gallery --release`.
2. Capture the window:
   - **macOS**: `Cmd+Shift+4`, then `Space`, then click the `mkui` window
     — or from a terminal, `screencapture -w component-gallery.png` and
     click the window when prompted. Requires Screen Recording permission
     for the terminal/IDE running the command (System Settings → Privacy
     & Security → Screen Recording).
   - **Linux**: `gnome-screenshot -w -f component-gallery.png` (GNOME) or
     your window manager's equivalent windowed-capture binding.
   - **Windows**: `Win+Shift+S`, select the window region, paste into an
     image editor and save as `component-gallery.png`.
3. Name the file after the section(s) it's meant to document (e.g.
   `component-gallery-button.png`, `component-gallery-card.png`) and
   attach it to the PR or issue comment it supports, rather than
   committing it to the repo — the gallery itself is the reproducible
   source of truth; a screenshot is a point-in-time illustration of it.

This matches the workflow #278's web/shadcn-parity fixture documents for
its own side-by-side screenshots, so the two fixtures stay comparable in
process even though this one has no external reference image to diff
against.

## Growing this gallery

As [#270](https://github.com/mikbry/ui/issues/270)'s promotion programme
lifts more components into the shared `mkui-core` contract, add that
component's section to `create_gallery_ui` in
[`examples/showcase-common/src/lib.rs`](../showcase-common/src/lib.rs) in
the same PR — see the doc comment on `create_gallery_ui` for the
convention each section follows (a heading naming the component and its
variants, then the variants rendered side by side).
