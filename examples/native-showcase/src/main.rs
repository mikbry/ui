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
//!
//! With the (default-on) `slug` feature, `main` below also selects a Slug
//! font before `.run()` (#311) — the declarative-bridge counterpart of
//! `examples/host-embed-wgpu`'s hand-wired `EmbeddedRenderer::set_slug_font`
//! call (#259). This can no longer go through the one-line `mkui::run!`
//! macro, since that macro only calls `.run()` with no seam to select a
//! font in between; `main` below is `run!`'s own `wgpu` arm, expanded by
//! hand with the Slug step inserted.

use showcase_common::create_showcase_ui;

/// Abel (Google Fonts, SIL Open Font License 1.1) — embedded for this
/// example only, mirroring `examples/host-embed-wgpu`'s own per-example
/// embed (#259); never bundled into the `mkui-wgpu` library itself (font
/// choice is a host/app decision, not a library one). Only referenced
/// under `slug`; an off-feature (`--no-default-features --features wgpu`)
/// build never decodes it and stays on the original bitmap lane.
#[cfg(feature = "slug")]
const ABEL_TTF: &[u8] = include_bytes!("../../../crates/text/tests/fixtures/abel/Abel-Regular.ttf");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = match create_showcase_ui() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Failed to create app: {}", e);
            return Err(Box::<dyn std::error::Error>::from(e.to_string()));
        }
    };

    #[cfg(feature = "slug")]
    let app = {
        let mut text_system = mkui::CompositeTextSystem::new();
        let font_id = text_system
            .register_sfnt_face(
                std::sync::Arc::from(ABEL_TTF.to_vec().into_boxed_slice()),
                0,
            )
            .expect("Abel registers");
        match app.with_slug_font(text_system, font_id) {
            Ok(app) => app,
            Err(e) => {
                eprintln!("Failed to select Slug font: {}", e);
                return Err(Box::<dyn std::error::Error>::from(e.to_string()));
            }
        }
    };

    match app.run() {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!("Failed to run app: {}", e);
            Err(Box::<dyn std::error::Error>::from(e.to_string()))
        }
    }
}
