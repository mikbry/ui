//! Atoms-on-wgpu visual integration example.
//!
//! Combines the three Sprint 2 deliverables in one scene:
//!
//! - `mkui-text`'s `BitmapTextSystem` (exercised by every `Text`
//!   primitive — the default text system inside `WgpuApp` handles the
//!   5×7 ASCII bitmap rasterization end-to-end).
//! - `mkui-wgpu`'s `Mkui::run()` HUD renderer (real `wgpu::Surface` +
//!   `winit::ApplicationHandler` shell).
//! - `mkui-wgpu`'s `Badge` (6 shadcn variants × 2 sizes) and `Dot` (4
//!   status variants × sizes × halo × animations) atoms.
//!
//! Run windowed: `cargo run -p atoms-on-wgpu`
//! Run headless: `HEADLESS=1 cargo run -p atoms-on-wgpu`
//!
//! The headless path skips the winit window and asserts the scene
//! contains the expected primitive counts so CI can keep the visual
//! integration smoke green on headless runners.

use mkui_wgpu::components::{badge, dot};
use mkui_wgpu::prelude::*;
use mkui_wgpu::types::{Primitive, Text};

const VIEWPORT: Size = Size::new(820.0, 520.0);

const BADGE_VARIANTS: [(&str, BadgeVariant); 6] = [
    ("default", BadgeVariant::Default),
    ("destructive", BadgeVariant::Destructive),
    ("outline", BadgeVariant::Outline),
    ("secondary", BadgeVariant::Secondary),
    ("ghost", BadgeVariant::Ghost),
    ("link", BadgeVariant::Link),
];

const DOT_VARIANTS: [DotVariant; 4] = [
    DotVariant::Ok,
    DotVariant::Warn,
    DotVariant::Danger,
    DotVariant::Neutral,
];

const ANIMATIONS: [DotAnimation; 3] = [
    DotAnimation::Pulse,
    DotAnimation::PulseUrgent,
    DotAnimation::Spin,
];

fn build_scene() -> Scene {
    let theme = HudTheme::default();
    let mut scene = Scene::new(VIEWPORT);

    // Title block — drives the BitmapTextSystem path end-to-end.
    scene.text(Text {
        rect: Rect::new(
            Point::new(24.0, 18.0),
            Size::new(VIEWPORT.width - 48.0, 36.0),
        ),
        content: "mkui v0.5.0 — atoms on wgpu".to_string(),
        style: theme.title_style,
    });

    // Badge grid: 6 variants × 2 sizes laid out as a 6-column, 2-row table.
    let badge_grid_top = 80.0;
    let badge_col_w = 124.0;
    let badge_row_gap = 44.0;
    for (col, (label, variant)) in BADGE_VARIANTS.iter().enumerate() {
        let x = 28.0 + col as f32 * badge_col_w;
        badge(
            &mut scene,
            Rect::new(Point::new(x, badge_grid_top), Size::new(108.0, 26.0)),
            *label,
            *variant,
            BadgeSize::Default,
            &theme,
        );
        badge(
            &mut scene,
            Rect::new(
                Point::new(x, badge_grid_top + badge_row_gap),
                Size::new(86.0, 18.0),
            ),
            *label,
            *variant,
            BadgeSize::Sm,
            &theme,
        );
    }

    // Dot showcase: per variant, three columns — Sm, Md, Md+halo.
    let dot_grid_top = 220.0;
    let dot_row_gap = 36.0;
    let dot_col_x = [80.0, 160.0, 240.0];
    for (row, variant) in DOT_VARIANTS.iter().enumerate() {
        let y = dot_grid_top + row as f32 * dot_row_gap;
        dot(
            &mut scene,
            Point::new(dot_col_x[0], y),
            *variant,
            DotSize::Sm,
            false,
            DotAnimation::None,
            &theme,
        );
        dot(
            &mut scene,
            Point::new(dot_col_x[1], y),
            *variant,
            DotSize::Md,
            false,
            DotAnimation::None,
            &theme,
        );
        dot(
            &mut scene,
            Point::new(dot_col_x[2], y),
            *variant,
            DotSize::Md,
            true,
            DotAnimation::None,
            &theme,
        );
    }

    // Animation row: one example per non-`None` animation kind, each on a
    // halo-modified Md dot so the motion primitive has a visible carrier.
    let anim_y = dot_grid_top + DOT_VARIANTS.len() as f32 * dot_row_gap + 24.0;
    for (i, kind) in ANIMATIONS.iter().enumerate() {
        dot(
            &mut scene,
            Point::new(80.0 + i as f32 * 80.0, anim_y),
            DotVariant::Ok,
            DotSize::Md,
            true,
            *kind,
            &theme,
        );
    }

    scene
}

fn count_primitives(scene: &Scene) -> (usize, usize) {
    let quads = scene
        .primitives
        .iter()
        .filter(|p| matches!(p, Primitive::Quad(_)))
        .count();
    let texts = scene
        .primitives
        .iter()
        .filter(|p| matches!(p, Primitive::Text(_)))
        .count();
    (quads, texts)
}

fn run_headless() -> Result<(), String> {
    let scene = build_scene();
    let (quads, texts) = count_primitives(&scene);
    let animations = scene.animations.len();

    // Lower bounds derived from the layout above:
    //   Badges:        12 background quads + 12 label texts
    //   Dot grid:      4 variants × (Sm + Md + Md/halo) = 12 dot bodies + 4 halos
    //   Animation row: 3 animations × (body + halo) = 6 quads, 3 instances
    //   Title:         1 text primitive
    // Use `>=` so a future theme tweak that adds a shadow underlay does not
    // break the smoke; the assertion is that the showcase is at least as
    // rich as the spec.
    let expected_quads = 12 + 12 + 4 + 6;
    let expected_texts = 12 + 1;
    let expected_animations = ANIMATIONS.len();

    if quads < expected_quads {
        return Err(format!(
            "expected >= {expected_quads} quad primitives, got {quads}"
        ));
    }
    if texts < expected_texts {
        return Err(format!(
            "expected >= {expected_texts} text primitives, got {texts}"
        ));
    }
    if animations != expected_animations {
        return Err(format!(
            "expected exactly {expected_animations} animations, got {animations}"
        ));
    }

    println!(
        "atoms-on-wgpu headless smoke OK — quads: {quads}, texts: {texts}, animations: {animations}"
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let headless =
        std::env::var_os("HEADLESS").is_some() || std::env::args().any(|arg| arg == "--headless");
    if headless {
        run_headless()?;
        return Ok(());
    }

    Mkui::with_scene(build_scene()).run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_scene_emits_full_showcase_inventory() {
        let scene = build_scene();
        let (quads, texts) = count_primitives(&scene);
        assert!(quads >= 34, "quad count: {quads}");
        assert!(texts >= 13, "text count: {texts}");
        assert_eq!(scene.animations.len(), ANIMATIONS.len());
    }

    #[test]
    fn headless_smoke_exits_ok() {
        run_headless().expect("headless smoke should pass");
    }
}
