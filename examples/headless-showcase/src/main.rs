use mkui_core::headless::{KeyboardInteractable, ToggleBuilder};
use mkui_wgpu::components::{badge, dot};
use mkui_wgpu::prelude::*;

fn main() {
    // ---- Headless toggle (existing demo) ------------------------------
    let mut toggle = ToggleBuilder::new()
        .checked(false)
        .on_change(|checked| println!("Toggle changed to: {checked}"))
        .build();
    println!("Initial state: {}", toggle.is_checked());
    toggle.toggle();
    println!("After toggle: {}", toggle.is_checked());
    toggle.handle_key_down(" ");
    println!("After space key: {}", toggle.is_checked());

    // ---- Atom showcase: Badge × variant × size + Dot × variant × halo × animation
    let theme = HudTheme::default();
    let mut scene = Scene::new(Size::new(640.0, 320.0));

    // Badge grid: each variant in both sizes, laid out as a 4×2 grid.
    let badge_variants = [
        ("default", BadgeVariant::Default),
        ("destructive", BadgeVariant::Destructive),
        ("outline", BadgeVariant::Outline),
        ("secondary", BadgeVariant::Secondary),
        ("ghost", BadgeVariant::Ghost),
        ("link", BadgeVariant::Link),
    ];
    for (col, (label, variant)) in badge_variants.iter().enumerate() {
        let x = 20.0 + col as f32 * 110.0;
        badge(
            &mut scene,
            Rect::new(Point::new(x, 20.0), Size::new(96.0, 22.0)),
            *label,
            *variant,
            BadgeSize::Default,
            &theme,
        );
        badge(
            &mut scene,
            Rect::new(Point::new(x, 56.0), Size::new(72.0, 14.0)),
            *label,
            *variant,
            BadgeSize::Sm,
            &theme,
        );
    }

    // Dot grid: each variant × halo × animation modifier.
    let dot_variants = [
        DotVariant::Ok,
        DotVariant::Warn,
        DotVariant::Danger,
        DotVariant::Neutral,
    ];
    let animations = [
        DotAnimation::None,
        DotAnimation::Pulse,
        DotAnimation::PulseUrgent,
        DotAnimation::Spin,
    ];
    for (row, variant) in dot_variants.iter().enumerate() {
        for (col, animation) in animations.iter().enumerate() {
            let y = 110.0 + row as f32 * 28.0;
            let x = 30.0 + col as f32 * 80.0;
            // no halo
            dot(
                &mut scene,
                Point::new(x, y),
                *variant,
                DotSize::Sm,
                false,
                *animation,
                &theme,
            );
            // with halo
            dot(
                &mut scene,
                Point::new(x + 30.0, y),
                *variant,
                DotSize::Md,
                true,
                *animation,
                &theme,
            );
        }
    }

    let quad_count = scene
        .primitives
        .iter()
        .filter(|p| matches!(p, mkui_wgpu::Primitive::Quad(_)))
        .count();
    let text_count = scene
        .primitives
        .iter()
        .filter(|p| matches!(p, mkui_wgpu::Primitive::Text(_)))
        .count();
    println!("Scene primitives: {} total", scene.primitives.len());
    println!("  Quads:      {quad_count}");
    println!("  Texts:      {text_count}");
    println!("  Animations: {}", scene.animations.len());
}
