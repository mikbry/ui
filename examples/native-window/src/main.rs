//! Minimal native-window example.
//!
//! Opens a winit window via `Mkui::run()` and paints a clear color plus a
//! single quad emitted by the existing `Scene` API. This is the smoke
//! test referenced by issue #20's acceptance criteria — the window
//! shouldn't show anything more elaborate than that, so any visual
//! regression in the HUD pipeline is immediately obvious.

use mkui_wgpu::{Color, CornerRadii, Mkui, Point, Primitive, Quad, Rect, Scene, Size};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let viewport = Size::new(800.0, 600.0);
    let mut scene = Scene::new(viewport);
    scene.push(Primitive::Quad(Quad {
        rect: Rect::new(Point::new(200.0, 150.0), Size::new(400.0, 300.0)),
        fill: Color::rgba(0.42, 0.66, 0.84, 1.0),
        corner_radii: CornerRadii::all(0.0),
        stroke: None,
    }));

    let mkui = Mkui::with_scene(scene);
    mkui.run()?;
    Ok(())
}
