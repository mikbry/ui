//! Native showcase — drives `examples/showcase-common::create_showcase_ui`
//! through the mkui-wgpu declarative bridge (issue #56 §7, AC #1 + #4).
//!
//! The showcase function itself is byte-unchanged from main (AC #3 —
//! audit-grade preservation per Codex round-7 Q6 ratification); this
//! binary is the wgpu-side consumer that proves the bridge runs the
//! shared tree end-to-end.
//!
//! `HEADLESS=1 cargo run -p native-showcase --release` exits
//! after a single walker pass — useful for CI smoke validation when
//! there is no display server.

use showcase_common::create_showcase_ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    mkui::run!(create_showcase_ui, wgpu)
}
