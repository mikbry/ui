//! Native component gallery (#310) — drives
//! `examples/showcase-common::create_gallery_ui` through the mkui-wgpu
//! declarative bridge, same shape as `examples/native-showcase`'s
//! `create_showcase_ui` consumer.
//!
//! This is the native (`mkui-wgpu`) counterpart to #278's web/shadcn
//! parity fixture: #278 compares a component against an external
//! reference, this example has none — its only job is to make every
//! component with a shared `mkui-core` contract representation visible
//! and inspectable on the native backend as #270's promotion programme
//! adds more of them.
//!
//! `HEADLESS=1 cargo run -p component-gallery --release` exits after a
//! single walker pass — useful for CI smoke validation when there is no
//! display server, matching `native-showcase`'s own `HEADLESS` contract.
//!
//! With the (default-on) `slug` feature, `main` below also selects a Slug
//! font before `.run()` (#330, mirroring #311's `native-showcase` wiring) —
//! this crate already built with `slug` default-on via its
//! `showcase-common` dependency, but rendered bitmap text at runtime
//! because nothing called `.with_slug_font()`. This can no longer go
//! through the one-line `mkui::run!` macro, since that macro only calls
//! `.run()` with no seam to select a font in between; `main` below is
//! `run!`'s own `wgpu` arm, expanded by hand with the Slug step inserted.

use showcase_common::create_gallery_ui;

/// Abel (Google Fonts, SIL Open Font License 1.1) — embedded for this
/// example only, mirroring `native-showcase`'s own per-example embed
/// (#311/#259); never bundled into the `mkui-wgpu` library itself (font
/// choice is a host/app decision, not a library one). Only referenced
/// under `slug`; an off-feature (`--no-default-features --features wgpu`)
/// build never decodes it and stays on the original bitmap lane.
#[cfg(feature = "slug")]
const ABEL_TTF: &[u8] = include_bytes!("../../../crates/text/tests/fixtures/abel/Abel-Regular.ttf");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app = match create_gallery_ui() {
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
