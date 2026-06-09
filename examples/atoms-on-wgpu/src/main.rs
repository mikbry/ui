//! Atoms-on-wgpu — declarative AppTree rendering of the wgpu badge + dot
//! atoms (re-introduction of the v0.5.x closed-not-merged #48 demo, now
//! via the runtime AppTree path — issue #56 §6).
//!
//! The tree is built directly with `tree.push_text` /
//! `tree.push_custom`; `WgpuRendererRegistry::with_defaults()` ships the
//! `BadgeRenderer` and `DotRenderer` so the `NodeKind::Custom` payloads
//! resolve without per-app wiring.
//!
//! `HEADLESS=1 cargo run --example atoms-on-wgpu` exits after a single
//! walker pass (acceptance criterion #18 — the headless smoke gate).

use mkui_core::components::Mkui as CoreMkui;
use mkui_runtime::{AppTree, TextVariant};
use mkui_wgpu::Mkui;
use serde_json::json;

const BADGE_VARIANTS: &[(&str, &str)] = &[
    ("default", "default"),
    ("destructive", "destructive"),
    ("outline", "outline"),
    ("secondary", "secondary"),
    ("ghost", "ghost"),
    ("link", "link"),
];

const DOT_VARIANTS: &[&str] = &["ok", "warn", "danger", "neutral"];
const DOT_ANIMATIONS: &[&str] = &["none", "pulse", "pulse_urgent", "spin"];

fn build_tree() -> AppTree {
    let mut tree = AppTree::new();
    let root = tree.root();

    // Title text — a regular built-in `Text` node, rendered through the
    // walker's fixed path. Position is the walker's default top-left.
    tree.push_text(
        root,
        "atoms-on-wgpu",
        TextVariant::Heading1,
        "text-4xl font-bold",
    )
    .expect("title text");

    // 12-badge grid (6 variants × 2 sizes). Coordinates are absolute so
    // the badges land in a recognisable grid without depending on the
    // walker's still-simple vertical-flow layout.
    let badge_y_default = 90.0_f32;
    let badge_y_sm = 130.0_f32;
    let badge_x_step = 120.0_f32;
    let badge_x_origin = 24.0_f32;
    for (col, (label, variant)) in BADGE_VARIANTS.iter().enumerate() {
        let x = badge_x_origin + col as f32 * badge_x_step;
        tree.push_custom(
            root,
            "badge",
            json!({
                "label": label,
                "variant": variant,
                "size": "default",
                "x": x,
                "y": badge_y_default,
                "width": 96.0,
                "height": 22.0,
            }),
            "",
        )
        .expect("badge default");
        tree.push_custom(
            root,
            "badge",
            json!({
                "label": label,
                "variant": variant,
                "size": "sm",
                "x": x,
                "y": badge_y_sm,
                "width": 72.0,
                "height": 14.0,
            }),
            "",
        )
        .expect("badge sm");
    }

    // Dot showcase — variant × animation grid, with halo + non-halo
    // columns. Mirrors the v0.5.x demo shape.
    let dot_y_origin = 200.0_f32;
    let dot_y_step = 28.0_f32;
    let dot_x_origin = 40.0_f32;
    let dot_x_step = 80.0_f32;
    for (row, variant) in DOT_VARIANTS.iter().enumerate() {
        for (col, animation) in DOT_ANIMATIONS.iter().enumerate() {
            let y = dot_y_origin + row as f32 * dot_y_step;
            let x = dot_x_origin + col as f32 * dot_x_step;
            tree.push_custom(
                root,
                "dot",
                json!({
                    "variant": variant,
                    "size": "sm",
                    "halo": false,
                    "animation": animation,
                    "x": x,
                    "y": y,
                }),
                "",
            )
            .expect("dot no-halo");
            tree.push_custom(
                root,
                "dot",
                json!({
                    "variant": variant,
                    "size": "md",
                    "halo": true,
                    "animation": animation,
                    "x": x + 30.0,
                    "y": y,
                }),
                "",
            )
            .expect("dot with-halo");
        }
    }

    tree
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tree = build_tree();
    let core = CoreMkui::with_tree(tree);
    let app = Mkui::from_core(core)?;
    app.run()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_runtime::NodeKind;
    use mkui_wgpu::{
        tessellate_scene, walk_app_tree, Size, WalkOptions, WgpuRendererRegistry, WgpuTheme,
    };

    #[test]
    fn build_tree_emits_title_and_twelve_badges_and_dots() {
        let tree = build_tree();
        let mut texts = 0;
        let mut badges = 0;
        let mut dots = 0;
        for node in tree.nodes() {
            match &node.kind {
                NodeKind::Text(_) => texts += 1,
                NodeKind::Custom { type_name, .. } if type_name == "badge" => badges += 1,
                NodeKind::Custom { type_name, .. } if type_name == "dot" => dots += 1,
                _ => {}
            }
        }
        assert_eq!(texts, 1, "single title text");
        assert_eq!(badges, 12, "6 variants × 2 sizes");
        // 4 variants × 4 animations × 2 (halo + no-halo) = 32 dots.
        assert_eq!(dots, 32);
    }

    #[test]
    fn tree_walks_through_default_registry_to_non_empty_render_input() {
        // #93 regression gate: the declarative tree must walk through the
        // default registry (BadgeRenderer + DotRenderer) into a non-empty
        // scene that tessellates to non-empty triangles — the displayless
        // proof that the GPU stage would draw. atoms-on-wgpu rendered empty
        // despite this producing thousands of triangles; the visual fault
        // was the MSAA resolve path (now disabled), not the walker.
        let core = CoreMkui::with_tree(build_tree());
        let registry = WgpuRendererRegistry::with_defaults();
        let options = WalkOptions {
            viewport: Size::new(1280.0, 720.0),
            theme: WgpuTheme::default(),
        };
        let out = walk_app_tree(core.tree(), &registry, &options).expect("walk");
        assert!(
            !out.scene.primitives.is_empty(),
            "walker must emit primitives for the badge/dot tree"
        );
        let triangles = tessellate_scene(&out.scene);
        assert!(
            !triangles.is_empty(),
            "tessellation must yield non-empty render input"
        );
    }
}
