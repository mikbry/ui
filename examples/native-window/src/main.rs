//! Minimal native-window example.
//!
//! Opens a winit window via `Mkui::run()` and paints a clear color plus a
//! single quad emitted by the existing `Scene` API. This is the smoke
//! test referenced by issue #20's acceptance criteria — the window
//! shouldn't show anything more elaborate than that, so any visual
//! regression in the render pipeline is immediately obvious.
//!
//! `cargo run -p native-window --release` opens the window. This is a
//! workspace package, so the `-p <package>` form is correct — not the
//! `cargo run --example <name>` form, which does not apply here.

use mkui_wgpu::{Color, CornerRadii, Mkui, Point, Primitive, Quad, Rect, Scene, Size};

/// Build the single-quad demo scene. Factored out of `main` so the
/// displayless render-input test (#93 regression gate) can assert it
/// tessellates to non-empty triangles without opening a window.
fn build_scene() -> Scene {
    let viewport = Size::new(800.0, 600.0);
    let mut scene = Scene::new(viewport);
    scene.push(Primitive::Quad(Quad {
        rect: Rect::new(Point::new(200.0, 150.0), Size::new(400.0, 300.0)),
        fill: Color::rgba(0.42, 0.66, 0.84, 1.0),
        corner_radii: CornerRadii::all(0.0),
        stroke: None,
    }));
    scene
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `with_scene` is the retained low-level escape hatch for the raw
    // `Scene` API (ADR 0006). The native-window smoke deliberately
    // exercises this direct-to-renderer path so a regression in the
    // tessellation pipeline still shows up here.
    let mkui = Mkui::with_scene(build_scene());
    mkui.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_wgpu::tessellate_scene;

    #[test]
    fn build_scene_tessellates_to_non_empty_triangles() {
        // #93 regression gate: the raw-scene quad must reach the GPU stage
        // as non-empty triangles. A tessellation regression (or an empty
        // scene) trips here without needing a display server.
        let scene = build_scene();
        assert_eq!(scene.primitives.len(), 1, "one quad pushed");
        let triangles = tessellate_scene(&scene);
        assert_eq!(triangles.len(), 2, "one quad → two triangles");
    }
}
