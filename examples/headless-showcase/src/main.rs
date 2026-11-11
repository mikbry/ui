use mkui_core::headless::{
    KeyboardInteractable, SegmentedSelectBuilder, SliderBuilder, ToggleBuilder,
};
use mkui_wgpu::components::{badge, dot, segmented_select, slider_with_label, toggle};
use mkui_wgpu::prelude::*;

fn main() {
    // ---- Headless toggle (existing demo) ------------------------------
    let mut headless_toggle = ToggleBuilder::new()
        .checked(false)
        .on_change(|checked| println!("Toggle changed to: {checked}"))
        .build();
    println!("Initial state: {}", headless_toggle.is_checked());
    headless_toggle.toggle();
    println!("After toggle: {}", headless_toggle.is_checked());
    headless_toggle.handle_key_down(" ");
    println!("After space key: {}", headless_toggle.is_checked());

    // ---- Headless slider + segmented select (#190) --------------------
    // Keyboard/gamepad operability lives entirely in the headless layer
    // (`mkui_core::headless`); the wgpu `slider_with_label` / `toggle` /
    // `segmented_select` draw functions below just render whatever state it
    // hands them — see each component module's doc comment.
    let mut headless_slider = SliderBuilder::new()
        .min(0.0)
        .max(100.0)
        .step(5.0)
        .value(50.0)
        .build();
    headless_slider.handle_key_down("ArrowRight");
    headless_slider.handle_key_down("ArrowRight");
    println!(
        "Slider value after two ArrowRight presses: {}",
        headless_slider.value()
    );

    let mut headless_segmented = SegmentedSelectBuilder::new().option_count(3).build();
    headless_segmented.handle_key_down("ArrowRight");
    println!(
        "Segmented selection after one ArrowRight press: {}",
        headless_segmented.selected()
    );

    // ---- Atom showcase: Badge × variant × size + Dot × variant × halo × animation
    let theme = WgpuTheme::default();
    let mut scene = Scene::new(Size::new(640.0, 320.0));

    toggle(
        &mut scene,
        Rect::new(Point::new(20.0, 150.0), Size::new(40.0, 20.0)),
        headless_toggle.is_checked(),
        &theme.toggle(),
    );
    slider_with_label(
        &mut scene,
        Rect::new(Point::new(80.0, 156.0), Size::new(160.0, 8.0)),
        headless_slider.fraction(),
        Some(&format!("{}", headless_slider.value())),
        &theme.slider(),
    );
    let mut layout: PanelLayout<usize> = PanelLayout::default();
    segmented_select(
        &mut scene,
        &mut layout,
        Rect::new(Point::new(280.0, 146.0), Size::new(150.0, 28.0)),
        &["EN", "FR", "DE"],
        headless_segmented.selected(),
        &theme.segmented_select(),
        |i| i,
    );

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
