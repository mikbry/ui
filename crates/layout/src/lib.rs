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

        fn measure_text_field(
            &self,
            value: &str,
            placeholder: &str,
            _style: &ResolvedStyle,
            max_width_px: Option<f32>,
        ) -> Measured {
            // A text field fills whatever width it's given, same as a real
            // `<input>`; with no definite width (a Row's flex-basis query)
            // report the wider of value/placeholder's natural width, fixed
            // height matching the `mkui-wgpu` theme's input box.
            match max_width_px {
                Some(width) => Measured::new(width, 28.0),
                None => {
                    let value_width = self.measure_str(value, None).size.width;
                    let placeholder_width = self.measure_str(placeholder, None).size.width;
                    Measured::new(value_width.max(placeholder_width), 28.0)
                }
            }
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

        // #328: the real `create_showcase_ui` wraps its page in a
        // `ScrollArea` rather than a plain `View` — mirrored here so this
        // fixture's layout math matches the shipped structure exactly
        // (same box-clamp-but-let-content-overflow contract either way,
        // per `layout_container`/`layout_column_children`, but a `View`
        // and a `ScrollArea` differ in their own column-shrink floor —
        // see `distribute_main_axis`'s doc comment — so this choice is
        // not cosmetic).
        let page = tree.push_scroll_area(root, "flex-1").unwrap();

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

        fn measure_text_field(
            &self,
            value: &str,
            placeholder: &str,
            style: &ResolvedStyle,
            max_width_px: Option<f32>,
        ) -> Measured {
            match max_width_px {
                Some(width) => Measured::new(width, 28.0),
                None => {
                    let value_width = self
                        .measure_text(value, TextVariant::Body, style, None)
                        .size
                        .width;
                    let placeholder_width = self
                        .measure_text(placeholder, TextVariant::Body, style, None)
                        .size
                        .width;
                    Measured::new(value_width.max(placeholder_width), 28.0)
                }
            }
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

    #[test]
    fn image_leaf_takes_its_intrinsic_size_without_flex_grow() {
        // #196: an `Image` with no `flex-1` gets exactly its own
        // intrinsic width/height as its final box — `avail_width` for a
        // non-growing Row child equals its natural width (see
        // `layout_row_children`'s basis-then-distribute shape), so this
        // is the common case, not a special one.
        let mut tree = AppTree::new();
        let root = tree.root();
        let row = tree.push_view(root, "flex").unwrap();
        let image = tree
            .push_image(
                row,
                mkui_runtime::ImageSource::Texture(mkui_runtime::TextureHandle(1)),
                mkui_runtime::ImageFit::Cover,
                40,
                20,
                "",
            )
            .unwrap();
        let measure = FakeMeasure::new();
        let result = compute_layout(&tree, Size::new(400.0, 300.0), &measure);
        let rect = result.get(image).unwrap();
        assert_eq!(rect.size.width, 40.0);
        assert_eq!(rect.size.height, 20.0);
    }

    #[test]
    fn flex_grow_image_fills_the_rows_free_width_but_keeps_its_intrinsic_height() {
        // #196: `flex-1` is the (only) mechanism that gives an `Image`'s
        // box a different aspect ratio than its own intrinsic one — the
        // exact scenario `ImageFit::Contain`/`Cover` exist to handle.
        // `layout_subtree`'s `Image` arm uses `avail_width` (the row's
        // already-distributed per-child budget) for width while keeping
        // the intrinsic height, so growing past the 40px natural width
        // must not also grow the 20px height.
        let mut tree = AppTree::new();
        let root = tree.root();
        let row = tree.push_view(root, "flex").unwrap();
        let image = tree
            .push_image(
                row,
                mkui_runtime::ImageSource::Texture(mkui_runtime::TextureHandle(1)),
                mkui_runtime::ImageFit::Contain,
                40,
                20,
                "flex-1",
            )
            .unwrap();
        let measure = FakeMeasure::new();
        let result = compute_layout(&tree, Size::new(400.0, 300.0), &measure);
        let rect = result.get(image).unwrap();
        assert!(
            rect.size.width > 40.0,
            "flex-1 must grow past the intrinsic width, got {}",
            rect.size.width
        );
        assert_eq!(
            rect.size.height, 20.0,
            "height stays the image's own intrinsic height"
        );
    }

    #[test]
    fn column_children_overflow_instead_of_overlapping_when_shorter_than_content() {
        // #328: two `View` siblings, each with real text content, whose
        // combined natural height exceeds a deliberately short viewport.
        // Before the fix, `distribute_main_axis` shrank both proportionally
        // with no floor, compressing each below its own text's required
        // height — the text inside then overflowed its own box downward,
        // past the next sibling's top, producing visible overlap. The
        // fix's contract: a `View` never shrinks below its subtree's own
        // natural height, so the second sibling's top must never land
        // above the first sibling's bottom, no matter how short the
        // viewport is.
        let mut tree = AppTree::new();
        let root = tree.root();
        let first = tree.push_view(root, "").unwrap();
        tree.push_text(
            first,
            "First section heading and a line of body copy under it",
            TextVariant::Heading2,
            "",
        )
        .unwrap();
        let second = tree.push_view(root, "").unwrap();
        tree.push_text(
            second,
            "Second section heading and a line of body copy under it",
            TextVariant::Heading2,
            "",
        )
        .unwrap();

        let measure = FakeMeasure::new();
        // Each `View`'s natural (unwrapped) text line is far taller than
        // this combined budget once both are present.
        let result = compute_layout(&tree, Size::new(200.0, 10.0), &measure);

        let first_rect = result.get(first).unwrap();
        let second_rect = result.get(second).unwrap();
        assert!(
            first_rect.bottom() <= second_rect.origin.y + 0.01,
            "siblings must never overlap: first bottom {} vs second top {}",
            first_rect.bottom(),
            second_rect.origin.y
        );

        // Each `View`'s own box must still be at least as tall as its
        // child's natural content height — the floor the fix adds.
        let first_text = result
            .iter()
            .find(|(id, _)| tree.get(*id).unwrap().parent == Some(first))
            .map(|(_, r)| r)
            .unwrap();
        assert!(
            first_rect.size.height + 0.01 >= first_text.size.height,
            "a View must not shrink below its own subtree's required height"
        );
    }

    #[test]
    fn canonical_showcase_tree_does_not_overlap_at_a_short_viewport() {
        // Same contract as the minimal repro above, exercised against the
        // full canonical showcase tree (header / hero / card / footer) at
        // a viewport far shorter than its natural content height.
        let showcase = build_showcase_tree();
        let measure = FakeMeasure::new();
        let result = compute_layout(&showcase.tree, Size::new(800.0, 300.0), &measure);

        let header_row = result.get(showcase.header_row).unwrap();
        let main = result.get(showcase.main_container).unwrap();
        assert!(
            header_row.bottom() <= main.origin.y + 0.01,
            "header must stay above the main content even when the window is too short: \
             header bottom {} vs main top {}",
            header_row.bottom(),
            main.origin.y
        );

        let heading = result.get(showcase.hero_heading).unwrap();
        assert!(
            heading.origin.y >= main.origin.y - 0.01,
            "hero heading must stay inside the main container's top edge"
        );
    }

    #[test]
    fn explicit_height_scroll_area_keeps_its_shrunk_size_not_its_h_class() {
        // #341 regression: a `ScrollArea` with an explicit `h-16` (64px)
        // class, shrunk to 36px by `distribute_main_axis` because a 64px
        // sibling leaves only 36px of a 100px viewport, must report its
        // own box at 36px — not silently revert to the explicit 64px on
        // `layout_column_children`'s re-resolve pass, which would push
        // its bottom edge 28px past the viewport.
        let mut tree = AppTree::new();
        let root = tree.root();
        let sibling = tree.push_view(root, "h-16").unwrap();
        let scroll = tree.push_scroll_area(root, "h-16").unwrap();

        let measure = FakeMeasure::new();
        let result = compute_layout(&tree, Size::new(200.0, 100.0), &measure);

        let sibling_rect = result.get(sibling).unwrap();
        let scroll_rect = result.get(scroll).unwrap();

        assert!(
            (sibling_rect.size.height - 64.0).abs() < 0.01,
            "the plain View sibling's own explicit height must be unaffected: got {}",
            sibling_rect.size.height
        );
        assert!(
            (scroll_rect.size.height - 36.0).abs() < 0.01,
            "the ScrollArea must keep the shrunk 36px the column assigned it, \
             not revert to its own explicit h-16 (64px): got {}",
            scroll_rect.size.height
        );
        assert!(
            scroll_rect.bottom() <= 100.0 + 0.01,
            "the ScrollArea's box must stay within the 100px viewport: bottom at {}",
            scroll_rect.bottom()
        );
    }

    #[test]
    fn explicit_height_scroll_area_shrinks_its_flex_1_descendant_too() {
        // #341 round-2 regression: forcing the ScrollArea's own
        // `.size.height` back to 36px (the test above) is not enough on
        // its own — a `flex-1` descendant *inside* that ScrollArea must
        // also be laid out against the 36px budget, not the box's own
        // explicit `h-16` (64px), or it overflows past the ScrollArea's
        // own corrected box. Same 100px viewport / 64px sibling / h-16
        // ScrollArea as above, but with an empty `flex-1` View inside the
        // ScrollArea standing in for arbitrary nested content.
        let mut tree = AppTree::new();
        let root = tree.root();
        let _sibling = tree.push_view(root, "h-16").unwrap();
        let scroll = tree.push_scroll_area(root, "h-16").unwrap();
        let inner = tree.push_view(scroll, "flex-1").unwrap();

        let measure = FakeMeasure::new();
        let result = compute_layout(&tree, Size::new(200.0, 100.0), &measure);

        let scroll_rect = result.get(scroll).unwrap();
        let inner_rect = result.get(inner).unwrap();

        assert!(
            (scroll_rect.size.height - 36.0).abs() < 0.01,
            "the ScrollArea must still be shrunk to 36px: got {}",
            scroll_rect.size.height
        );
        assert!(
            (inner_rect.size.height - 36.0).abs() < 0.01,
            "the flex-1 descendant must be laid out against the ScrollArea's \
             real 36px budget, not its explicit h-16 (64px): got {}",
            inner_rect.size.height
        );
        assert!(
            inner_rect.bottom() <= 100.0 + 0.01,
            "the flex-1 descendant's box must stay within the 100px viewport: bottom at {}",
            inner_rect.bottom()
        );
    }

    #[test]
    fn scroll_area_shrink_floor_is_zero_unlike_a_plain_view_sibling() {
        // #328: the one case where `ChildPlan::shrink_to_zero` actually
        // changes the outcome. A `View` sibling (natural height ~60px: 3
        // short wrapped lines at this width) and a `ScrollArea` sibling
        // (natural height ~200px: a much longer run of text) together
        // overflow a 100px viewport. The `View`'s floor is its own
        // natural height, so it must come out unchanged at ~60px; the
        // `ScrollArea`'s floor is zero, so it must absorb the entire
        // shortfall instead, landing at `100 - 60 = 40px` — not shared
        // proportionally the way two plain `View`s would split it.
        let mut tree = AppTree::new();
        let root = tree.root();
        let view = tree.push_view(root, "").unwrap();
        tree.push_text(
            view,
            "Three short lines of wrapped text sit here for the view",
            TextVariant::Body,
            "",
        )
        .unwrap();
        let scroll = tree.push_scroll_area(root, "").unwrap();
        tree.push_text(
            scroll,
            "A much longer run of text that wraps across many more lines \
             than the plain view above, so its natural content height is \
             far taller than the view's own and than the viewport itself",
            TextVariant::Body,
            "",
        )
        .unwrap();

        let measure = FakeMeasure::new();
        // Narrow width so both text blocks wrap into multiple lines; the
        // `View`'s natural height alone is already well under 100px, and
        // the `ScrollArea`'s is well over it.
        let result = compute_layout(&tree, Size::new(120.0, 100.0), &measure);

        let view_rect = result.get(view).unwrap();
        let scroll_rect = result.get(scroll).unwrap();

        // Sanity: without the budget constraint, the View's natural
        // height is far below 100px and the ScrollArea's is far above —
        // otherwise this test wouldn't actually exercise the shrink path
        // the way it's meant to.
        let natural = compute_layout(&tree, Size::new(120.0, 10_000.0), &measure);
        let view_natural = natural.get(view).unwrap().size.height;
        let scroll_natural = natural.get(scroll).unwrap().size.height;
        assert!(view_natural > 0.0 && view_natural < 100.0);
        assert!(scroll_natural > 100.0);

        assert!(
            (view_rect.size.height - view_natural).abs() < 0.01,
            "a plain View must never shrink below its own natural height: \
             natural {view_natural} vs got {}",
            view_rect.size.height
        );
        assert!(
            (scroll_rect.size.height - (100.0 - view_natural)).abs() < 0.01,
            "a ScrollArea's floor is zero, so it alone must absorb the \
             shortfall the View's unshrinkable floor leaves behind: \
             expected {} vs got {}",
            100.0 - view_natural,
            scroll_rect.size.height
        );
    }

    #[test]
    fn scroll_area_box_clamps_to_the_viewport_while_its_child_keeps_its_natural_height() {
        // #328: a `ScrollArea` wrapping content taller than the viewport
        // must still report its own box at the viewport's height (so a
        // sibling laid out after it, or the window itself, is not pushed
        // around) while its child keeps its full natural height — the
        // overflow a scrollbar/clip would need to act on, not something
        // silently compressed away.
        let mut tree = AppTree::new();
        let root = tree.root();
        let scroll = tree.push_scroll_area(root, "flex-1").unwrap();
        tree.push_text(
            scroll,
            "A very long line of body copy that is much taller than the tiny viewport below",
            TextVariant::Body,
            "",
        )
        .unwrap();

        let measure = FakeMeasure::new();
        let result = compute_layout(&tree, Size::new(200.0, 50.0), &measure);

        let scroll_rect = result.get(scroll).unwrap();
        assert!(
            (scroll_rect.size.height - 50.0).abs() < 0.01,
            "ScrollArea's own box must clamp to the viewport budget, got {}",
            scroll_rect.size.height
        );

        let child = result
            .iter()
            .find(|(id, _)| tree.get(*id).unwrap().parent == Some(scroll))
            .map(|(_, r)| r)
            .unwrap();
        assert!(
            child.size.height > scroll_rect.size.height,
            "the child must keep its own natural (overflowing) height \
             instead of being shrunk to fit the box: child {} vs box {}",
            child.size.height,
            scroll_rect.size.height
        );
    }

    #[test]
    fn scroll_area_child_fills_a_viewport_taller_than_its_natural_content() {
        // #328: the mirror case — content shorter than the viewport.
        // `flex-1` on the `ScrollArea` must still let it (and transitively
        // its child, via the same re-resolve-on-grow pass every other
        // container uses) fill the available height, so large-window
        // layouts are unchanged from before this promotion.
        let mut tree = AppTree::new();
        let root = tree.root();
        let scroll = tree.push_scroll_area(root, "flex-1").unwrap();
        let inner = tree.push_view(scroll, "flex-1").unwrap();
        tree.push_text(inner, "short", TextVariant::Body, "")
            .unwrap();

        let measure = FakeMeasure::new();
        let result = compute_layout(&tree, Size::new(200.0, 400.0), &measure);

        let scroll_rect = result.get(scroll).unwrap();
        assert!(
            (scroll_rect.size.height - 400.0).abs() < 0.01,
            "ScrollArea must still grow to fill the viewport when content is short, got {}",
            scroll_rect.size.height
        );
        let inner_rect = result.get(inner).unwrap();
        assert!(
            (inner_rect.size.height - 400.0).abs() < 0.01,
            "a flex-1 descendant inside a short ScrollArea must still fill the viewport, got {}",
            inner_rect.size.height
        );
    }

    #[test]
    fn layout_result_translate_shifts_only_the_named_node() {
        // #328: `LayoutResult::translate` is the hook a host backend uses
        // to apply a `ScrollArea`'s live scroll offset post-layout. It
        // must move only the node it's given, leaving every other
        // already-computed rect (siblings, the node's own ancestors)
        // untouched.
        let mut tree = AppTree::new();
        let root = tree.root();
        let a = tree.push_view(root, "").unwrap();
        tree.push_text(a, "a", TextVariant::Body, "").unwrap();
        let b = tree.push_view(root, "").unwrap();
        tree.push_text(b, "b", TextVariant::Body, "").unwrap();

        let measure = FakeMeasure::new();
        let mut result = compute_layout(&tree, Size::new(200.0, 200.0), &measure);
        let a_before = result.get(a).unwrap();
        let b_before = result.get(b).unwrap();

        result.translate(a, Point::new(0.0, -15.0));

        let a_after = result.get(a).unwrap();
        assert!((a_after.origin.y - (a_before.origin.y - 15.0)).abs() < 0.001);
        assert_eq!(a_after.size, a_before.size, "translate must not resize");
        assert_eq!(
            result.get(b).unwrap(),
            b_before,
            "an untranslated sibling must be unaffected"
        );
    }

    #[test]
    fn image_without_flex_grow_keeps_its_intrinsic_width_under_a_stretch_aligned_column() {
        // Regression: a `Column`'s default `Align::Stretch` hands *every*
        // child the full `inner_width` as `avail_width` regardless of
        // child kind (see `layout_column_children`) — without the
        // `flex_grow` gate in the `Image` arm, a plain `Image` dropped
        // into an ordinary column (no `flex-1`) would silently stretch to
        // the column's full width instead of staying at its own 40px
        // intrinsic size.
        let mut tree = AppTree::new();
        let root = tree.root();
        let column = tree.push_view(root, "").unwrap(); // default direction is Column
        let image = tree
            .push_image(
                column,
                mkui_runtime::ImageSource::Texture(mkui_runtime::TextureHandle(1)),
                mkui_runtime::ImageFit::Cover,
                40,
                20,
                "",
            )
            .unwrap();
        let measure = FakeMeasure::new();
        // A viewport far wider than the 40px intrinsic width, so a bug
        // that stretches to `inner_width` is unmistakable.
        let result = compute_layout(&tree, Size::new(400.0, 300.0), &measure);
        let rect = result.get(image).unwrap();
        assert_eq!(
            rect.size.width, 40.0,
            "an Image with no flex-1 must keep its intrinsic width even under a stretch column"
        );
        assert_eq!(rect.size.height, 20.0);
    }
}
