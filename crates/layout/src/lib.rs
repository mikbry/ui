#![forbid(unsafe_code)]
//! # mkui-layout — backend-neutral flex layout engine over `AppTree`
//!
//! Issue #63 (F1 of the mkui goal/course-correction foundations, dated
//! 2026-11-16): Sprint 5's `mkui-wgpu` walker shipped a deliberately small,
//! wgpu-local layout pass (top-down vertical flow — see that crate's
//! `walker.rs` module docs) as a bridge move, not a permanent architecture.
//! This crate is the shared substrate that move always pointed at: it
//! consumes [`mkui_runtime::AppTree`] + [`mkui_runtime::ResolvedStyle`]
//! directly, measures `Text`/`Button`/`Custom` leaves through a
//! caller-supplied [`LeafMeasure`] callback (never shaping a glyph itself),
//! and returns computed border-box rects keyed by
//! [`mkui_runtime::NodeId`] — usable by any backend, not just wgpu.
//!
//! ## What's in scope today
//!
//! A from-scratch flex algorithm (row/column, wrap, grow/shrink,
//! justify/align, gap, min/max) over the token set `mkui-runtime`'s class
//! parser already resolves: `flex`, `flex-col`, `flex-wrap`, `flex-1`,
//! `items-center`, `justify-between`/`justify-center`, `gap-*`/`space-x-*`/
//! `space-y-*`, padding/margin tokens, `container`/`max-w-4xl`/`h-16`/
//! `h-auto`, and `hidden`. See [`flex`]'s module docs for the algorithm
//! itself and the correctness tolerances/gaps it names explicitly.
//!
//! ## What's deliberately not here
//!
//! No renderer, device, or surface type appears anywhere in this crate
//! (`grep -rn "wgpu::\|web_sys::\|crossterm::" crates/layout/src` must
//! stay empty) — a consuming backend converts this crate's own
//! [`Point`]/[`Size`]/[`Rect`] into its own geometry at the call site.
//!
//! ## Status: the `mkui-wgpu` walker calls this crate (#287)
//!
//! "The wgpu walker can consume computed rects instead of owning permanent
//! layout logic" (issue #63 acceptance criterion) started as a claim about
//! *compatibility* only, proven by `mkui-wgpu`'s own
//! `tests/mkui_layout_interop.rs` (originally a dev-dependency test: this
//! crate laying out the canonical showcase tree with a measurer that wraps
//! the walker's own `&dyn mkui_text::TextSystem`, then converting the
//! resulting rects into `mkui_wgpu::types::Rect`). Issue #287 did the real
//! surgery that claim deferred: `mkui-wgpu`'s `walker.rs` now calls
//! [`compute_layout`] directly via its own `WgpuLeafMeasure` (promoted from
//! that interop test's sketch), and its `WalkCache` was redesigned to key
//! subtree reuse on each node's own computed rect instead of the
//! `cursor_y`/`content_x`/`content_width` triple a combined render-and-
//! measure pass used to produce. Falsifiable check:
//! `grep -n "mkui_layout" crates/wgpu/src/walker.rs` returns the
//! walker's own call into [`compute_layout`] (in `walk_app_tree_inner`), not
//! just this module doc comment's own mentions.

pub mod flex;
pub mod geometry;
pub mod measure;

pub use flex::{compute_layout, LayoutResult};
pub use geometry::{Point, Rect, Size};
pub use measure::{LeafMeasure, Measured};

#[cfg(test)]
mod tests {
    use mkui_runtime::{AppTree, ButtonVariant, NodeId, ResolvedStyle, TextVariant};
    use mkui_text::{
        BitmapTextSystem, FontId, HintingMode, LayoutSpec, TextSystem, VariationSettings,
    };

    use super::*;

    /// Deterministic fake measurer — exact-pixel assertions on the
    /// canonical showcase tree must not depend on real glyph metrics, which
    /// can legitimately change as `mkui-text` grows. 8px per character,
    /// wraps greedily at whole characters when a `max_width_px` is given.
    struct FakeMeasure {
        char_width: f32,
        line_height: f32,
    }

    impl FakeMeasure {
        fn new() -> Self {
            Self {
                char_width: 8.0,
                line_height: 20.0,
            }
        }

        fn measure_str(&self, s: &str, max_width_px: Option<f32>) -> Measured {
            let chars = s.chars().count().max(1) as f32;
            let natural_width = chars * self.char_width;
            let Some(max_width) = max_width_px else {
                return Measured::new(natural_width, self.line_height);
            };
            if max_width <= 0.0 || natural_width <= max_width {
                return Measured::new(
                    natural_width.min(max_width.max(natural_width)),
                    self.line_height,
                );
            }
            let per_line = (max_width / self.char_width).floor().max(1.0);
            let lines = (chars / per_line).ceil().max(1.0);
            Measured::new(max_width, self.line_height * lines)
        }
    }

    impl LeafMeasure for FakeMeasure {
        fn measure_text(
            &self,
            content: &str,
            _variant: TextVariant,
            _style: &ResolvedStyle,
            max_width_px: Option<f32>,
        ) -> Measured {
            self.measure_str(content, max_width_px)
        }

        fn measure_button(
            &self,
            label: &str,
            _variant: ButtonVariant,
            _style: &ResolvedStyle,
            _max_width_px: Option<f32>,
        ) -> Measured {
            // Buttons never wrap — pad the label's natural width, fixed
            // height, same shape as the `mkui-wgpu` theme's button box.
            let label_width = self.measure_str(label, None).size.width;
            Measured::new(label_width + 32.0, 36.0)
        }
    }

    /// Rebuilds `examples/showcase-common::create_showcase_ui`'s structure
    /// directly against `mkui_runtime::AppTree` (the same level
    /// `mkui-wgpu`'s own walker tests build at) rather than depending on
    /// the `showcase-common`/`mkui` crates, which enable a primary backend
    /// feature on the `mkui` bridge and would risk Cargo feature
    /// unification tripping the workspace's one-backend `compile_error!`
    /// guard (see the root `Cargo.toml`'s `default-members` comment).
    struct ShowcaseTree {
        tree: AppTree,
        header_row: NodeId,
        header_logo: NodeId,
        header_caption: NodeId,
        header_github: NodeId,
        main_container: NodeId,
        hero_heading: NodeId,
        buttons: [NodeId; 5],
    }

    fn build_showcase_tree() -> ShowcaseTree {
        let mut tree = AppTree::new();
        let root = tree.root();

        let page = tree.push_view(root, "flex-1").unwrap();

        // Header
        let header = tree.push_view(page, "border-b").unwrap();
        let header_row = tree
            .push_view(
                header,
                "container mx-auto px-4 h-16 flex items-center justify-between",
            )
            .unwrap();
        let logo_group = tree
            .push_view(header_row, "flex items-center space-x-4")
            .unwrap();
        let header_logo = tree
            .push_text(
                logo_group,
                "miklabs/ui",
                TextVariant::Heading2,
                "text-xl font-semibold",
            )
            .unwrap();
        let header_caption = tree
            .push_text(
                logo_group,
                "Modern UI toolkit for Rust, C, C++ & Python",
                TextVariant::Caption,
                "text-sm text-muted-foreground hidden sm:block",
            )
            .unwrap();
        let actions_group = tree
            .push_view(header_row, "flex items-center gap-4")
            .unwrap();
        let action = tree.actions_mut().register_local(|ctx| ctx.mark_dirty());
        let header_github = tree
            .push_button(
                actions_group,
                "GitHub",
                ButtonVariant::Ghost,
                "",
                Some(action),
            )
            .unwrap();

        // Main content
        let main = tree.push_view(page, "flex-1").unwrap();
        let main_container = tree
            .push_view(main, "container mx-auto py-8 px-4 max-w-4xl space-y-8")
            .unwrap();

        let hero = tree.push_view(main_container, "text-center mb-12").unwrap();
        let hero_heading = tree
            .push_text(
                hero,
                "miklabs/ui Component Library",
                TextVariant::Heading1,
                "text-4xl font-bold tracking-tight text-foreground mb-4",
            )
            .unwrap();
        tree.push_text(
            hero,
            "Beautiful, accessible components for Rust, C, C++ & Python",
            TextVariant::Caption,
            "text-xl text-muted-foreground",
        )
        .unwrap();

        let card = tree
            .push_view(
                main_container,
                "rounded-lg border bg-card text-card-foreground shadow-sm p-6",
            )
            .unwrap();
        let card_header = tree.push_view(card, "mb-6").unwrap();
        tree.push_text(
            card_header,
            "Button Components",
            TextVariant::Heading2,
            "text-2xl font-semibold leading-none tracking-tight",
        )
        .unwrap();
        tree.push_text(
            card_header,
            "Various button styles and variants",
            TextVariant::Caption,
            "text-sm text-muted-foreground mt-2",
        )
        .unwrap();
        let button_row = tree.push_view(card, "flex flex-wrap gap-4").unwrap();
        let button_specs = [
            ("Primary", ButtonVariant::Primary),
            ("Secondary", ButtonVariant::Secondary),
            ("Destructive", ButtonVariant::Destructive),
            ("Outline", ButtonVariant::Outline),
            ("Ghost", ButtonVariant::Ghost),
        ];
        let mut buttons = [NodeId::from_raw(0, 0); 5];
        for (i, (label, variant)) in button_specs.into_iter().enumerate() {
            buttons[i] = tree
                .push_button(button_row, label, variant, "", None)
                .unwrap();
        }

        // Footer
        let footer = tree.push_view(page, "border-t mt-auto").unwrap();
        let footer_row = tree
            .push_view(footer, "container mx-auto px-4 py-6")
            .unwrap();
        let footer_flex = tree
            .push_view(
                footer_row,
                "flex flex-col sm:flex-row items-center justify-between gap-4 text-sm text-muted-foreground",
            )
            .unwrap();
        tree.push_text(footer_flex, "Built with Rust", TextVariant::Caption, "")
            .unwrap();

        ShowcaseTree {
            tree,
            header_row,
            header_logo,
            header_caption,
            header_github,
            main_container,
            hero_heading,
            buttons,
        }
    }

    #[test]
    fn canonical_showcase_layout_lays_out_the_tree() {
        let showcase = build_showcase_tree();
        let measure = FakeMeasure::new();
        let result = compute_layout(&showcase.tree, Size::new(1200.0, 900.0), &measure);

        // AC: "At least one test lays out the canonical showcase tree."
        assert!(
            result.len() > 10,
            "expected the full showcase tree to be laid out"
        );

        // `h-16` on the header row resolves to a fixed 64px height.
        let header_row = result.get(showcase.header_row).unwrap();
        assert!((header_row.size.height - 64.0).abs() < 0.01);

        // `items-center` centers both flex groups within that 64px band: a
        // leaf's own height is well under 64px, so the github button's top
        // must sit strictly after the row's top and its bottom strictly
        // before the row's bottom.
        let github = result.get(showcase.header_github).unwrap();
        assert!(github.size.height < 64.0);
        assert!(github.origin.y > header_row.origin.y);
        assert!(github.bottom() < header_row.bottom());

        // `justify-between`: the logo group (left) sits strictly before the
        // github button (right) — they must not overlap on the main axis.
        let logo = result.get(showcase.header_logo).unwrap();
        assert!(github.origin.x > logo.right());

        // `hidden`: the header caption must have no entry at all.
        assert!(result.get(showcase.header_caption).is_none());

        // `max-w-4xl` + `mx-auto`: at a wide viewport the main container
        // clamps to 896px and centers within the 1200px-wide page.
        let main = result.get(showcase.main_container).unwrap();
        assert!((main.size.width - 896.0).abs() < 0.01);
        assert!(
            main.origin.x > 0.0,
            "mx-auto must center the clamped container"
        );

        // Every button in the wrap row got a rect.
        for id in showcase.buttons {
            assert!(result.get(id).is_some());
        }

        // The hero heading (`text-4xl`) must be present and non-zero sized.
        let heading = result.get(showcase.hero_heading).unwrap();
        assert!(heading.size.width > 0.0 && heading.size.height > 0.0);
    }

    #[test]
    fn max_w_4xl_container_goes_full_width_on_a_narrow_viewport() {
        let showcase = build_showcase_tree();
        let measure = FakeMeasure::new();
        let result = compute_layout(&showcase.tree, Size::new(500.0, 900.0), &measure);
        let main = result.get(showcase.main_container).unwrap();
        // Viewport (500) minus the page's own px-4 contributions is below
        // 896px, so `max-w-4xl` never actually clamps — the container
        // takes the full available width instead.
        assert!(main.size.width < 896.0);
        assert!(main.origin.x >= 0.0 && main.origin.x < 1.0);
    }

    #[test]
    fn buttons_wrap_onto_a_second_line_at_a_narrow_viewport() {
        let showcase = build_showcase_tree();
        let measure = FakeMeasure::new();
        // Narrow enough that five buttons (each ~96-176px wide via the fake
        // measurer) cannot fit on one line.
        let result = compute_layout(&showcase.tree, Size::new(360.0, 900.0), &measure);
        let first = result.get(showcase.buttons[0]).unwrap();
        let last = result.get(showcase.buttons[4]).unwrap();
        assert!(
            last.origin.y > first.origin.y,
            "expected flex-wrap to push the last button onto a later line"
        );
    }

    /// A real `mkui-text` leaf measurer — satisfies the AC that text leaves
    /// are measured through an explicit callback into `mkui-text`, wired to
    /// the permanent bitmap debug-fallback system (ADR 0002).
    struct BitmapMeasure {
        system: BitmapTextSystem,
    }

    impl LeafMeasure for BitmapMeasure {
        fn measure_text(
            &self,
            content: &str,
            _variant: TextVariant,
            _style: &ResolvedStyle,
            max_width_px: Option<f32>,
        ) -> Measured {
            let font_size_px = 16.0;
            let spec = LayoutSpec {
                font_id: FontId::BITMAP,
                font_generation: 0,
                font_size_px,
                line_height_px: (font_size_px * 1.25).max(font_size_px + 2.0),
                align: mkui_text::TextAlign::Start,
                max_lines: None,
                hinting: HintingMode::None,
                variations: VariationSettings::empty(),
                synthesis_flags: 0,
            };
            let runs = self
                .system
                .layout(content, &spec, max_width_px.map(|w| w.max(1.0)));
            let width = runs
                .iter()
                .flat_map(|r| r.glyphs.iter())
                .map(|g| g.x_px + g.x_advance_px)
                .fold(0.0_f32, f32::max);
            let height = spec.line_height_px * (runs.len().max(1) as f32);
            Measured::new(width, height)
        }

        fn measure_button(
            &self,
            label: &str,
            variant: ButtonVariant,
            style: &ResolvedStyle,
            _max_width_px: Option<f32>,
        ) -> Measured {
            let label_size = self
                .measure_text(label, TextVariant::Label, style, None)
                .size;
            let _ = variant;
            Measured::new(label_size.width + 32.0, 36.0)
        }
    }

    #[test]
    fn canonical_showcase_layout_with_real_bitmap_text_system() {
        let showcase = build_showcase_tree();
        let measure = BitmapMeasure {
            system: BitmapTextSystem::new(),
        };
        let result = compute_layout(&showcase.tree, Size::new(1200.0, 900.0), &measure);
        assert!(result.len() > 10);
        let heading = result.get(showcase.hero_heading).unwrap();
        assert!(heading.size.width > 0.0 && heading.size.height > 0.0);
    }
}
