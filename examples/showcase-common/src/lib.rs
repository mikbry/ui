use mkui::prelude::*;
use mkui_core::components::{Button, Card, Image, ImageFit, Text, TextField, View};
use mkui_core::headless::{ButtonVariant, TextVariant};

/// A tiny (8x8) orange/blue checkerboard, generated once at
/// `python3 -c` time (see the #196 PR body) rather than shipped as a
/// binary asset file — the showcase needs no real-world image, just
/// *some* decodable PNG bytes to prove the decoded-bytes source path end
/// to end across every backend (ADR 0015 step 7).
const CHECKERBOARD_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 8, 0, 0, 0, 8, 8, 6, 0,
    0, 0, 196, 15, 190, 139, 0, 0, 0, 30, 73, 68, 65, 84, 120, 218, 99, 120, 86, 167, 244, 31, 25,
    155, 204, 184, 141, 130, 25, 232, 160, 0, 93, 0, 93, 3, 29, 20, 0, 0, 34, 83, 165, 97, 145,
    176, 137, 73, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

// Create the common showcase UI structure
pub fn create_showcase_ui() -> Result<Mkui, MkuiError> {
    Ok(Mkui::new()?
        .child(
            View::new()
                .class("flex-1")
                .child(
                    // Header
                    View::new()
                        .class("border-b")
                        .child(
                            View::new()
                                .class("container mx-auto px-4 h-16 flex items-center justify-between")
                                .child(
                                    View::new()
                                        .class("flex items-center space-x-4")
                                        .child(
                                            Text::new("miklabs/ui")
                                                .variant(TextVariant::Heading2)
                                                .class("text-xl font-semibold")
                                        )
                                        .child(
                                            Text::new("Modern UI toolkit for Rust, C, C++ & Python")
                                                .variant(TextVariant::Caption)
                                                .class("text-sm text-muted-foreground hidden sm:block")
                                        )
                                )
                                .child(
                                    View::new()
                                        .class("flex items-center gap-4")
                                        .child(
                                            Button::new("GitHub")
                                                .class("hover:bg-accent hover:text-accent-foreground")
                                                .variant(ButtonVariant::Ghost)
                                                .on_press(|| {
                                                    #[cfg(feature = "web")]
                                                    {
                                                        web_sys::window()
                                                            .unwrap()
                                                            .open_with_url_and_target("https://github.com/mikbry/ui", "_blank")
                                                            .ok();
                                                    }
                                                    #[cfg(feature = "console")]
                                                    {
                                                        println!("GitHub link clicked!");
                                                    }
                                                    #[cfg(feature = "wgpu")]
                                                    {
                                                        println!("GitHub link clicked!");
                                                    }
                                                })
                                        )
                                )
                        )
                )
                .child(
                    // Main content
                    View::new()
                        .class("flex-1")
                        .child(
                            View::new()
                                .class("container mx-auto py-8 px-4 max-w-4xl space-y-8")
                                .child(
                                    // Hero section
                                    View::new()
                                        .class("text-center mb-12")
                                        .child(
                                            Text::new("miklabs/ui Component Library")
                                                .variant(TextVariant::Heading1)
                                                .class("text-4xl font-bold tracking-tight text-foreground mb-4")
                                        )
                                        .child(
                                            Text::new("Beautiful, accessible components for Rust, C, C++ & Python")
                                                .variant(TextVariant::Caption)
                                                .class("text-xl text-muted-foreground")
                                        )
                                )
                                .child(
                                    // Button showcase section — a real `Card` node (#270), the
                                    // first component promoted beyond `Button` per ADR 0015,
                                    // rather than a `View` carrying `bg-card`/`border`/
                                    // `rounded-lg` classes by hand.
                                    Card::new()
                                        .class("p-6")
                                        .child(
                                            View::new()
                                                .class("mb-6")
                                                .child(
                                                    Text::new("Button Components")
                                                        .variant(TextVariant::Heading2)
                                                        .class("text-2xl font-semibold leading-none tracking-tight")
                                                )
                                                .child(
                                                    Text::new("Various button styles and variants")
                                                        .variant(TextVariant::Caption)
                                                        .class("text-sm text-muted-foreground mt-2")
                                                )
                                        )
                                        .child(
                                            View::new()
                                                .class("flex flex-wrap gap-4")
                                                .child(
                                                    Button::new("Primary")
                                                        .variant(ButtonVariant::Primary)
                                                        .on_press(|| {
                                                            #[cfg(feature = "web")]
                                                            web_sys::console::log_1(&"Primary button clicked!".into());
                                                            #[cfg(feature = "console")]
                                                            println!("Primary button clicked!");
                                                            #[cfg(feature = "wgpu")]
                                                            println!("Primary button clicked!");
                                                        })
                                                )
                                                .child(
                                                    Button::new("Secondary")
                                                        .variant(ButtonVariant::Secondary)
                                                        .on_press(|| {
                                                            #[cfg(feature = "web")]
                                                            web_sys::console::log_1(&"Secondary button clicked!".into());
                                                            #[cfg(feature = "console")]
                                                            println!("Secondary button clicked!");
                                                            #[cfg(feature = "wgpu")]
                                                            println!("Secondary button clicked!");
                                                        })
                                                )
                                                .child(
                                                    Button::new("Destructive")
                                                        .variant(ButtonVariant::Destructive)
                                                        .on_press(|| {
                                                            #[cfg(feature = "web")]
                                                            web_sys::console::log_1(&"Destructive button clicked!".into());
                                                            #[cfg(feature = "console")]
                                                            println!("Destructive button clicked!");
                                                            #[cfg(feature = "wgpu")]
                                                            println!("Destructive button clicked!");
                                                        })
                                                )
                                                .child(
                                                    Button::new("Outline")
                                                        .variant(ButtonVariant::Outline)
                                                        .on_press(|| {
                                                            #[cfg(feature = "web")]
                                                            web_sys::console::log_1(&"Outline button clicked!".into());
                                                            #[cfg(feature = "console")]
                                                            println!("Outline button clicked!");
                                                            #[cfg(feature = "wgpu")]
                                                            println!("Outline button clicked!");
                                                        })
                                                )
                                                .child(
                                                    Button::new("Ghost")
                                                        .variant(ButtonVariant::Ghost)
                                                        .on_press(|| {
                                                            #[cfg(feature = "web")]
                                                            web_sys::console::log_1(&"Ghost button clicked!".into());
                                                            #[cfg(feature = "console")]
                                                            println!("Ghost button clicked!");
                                                            #[cfg(feature = "wgpu")]
                                                            println!("Ghost button clicked!");
                                                        })
                                                )
                                                .child(
                                                    // #245 fold-in: a disabled button, with no
                                                    // `on_press` at all (nothing to fire it with
                                                    // regardless) — the first time `disabled` is
                                                    // exercised through the canonical, every-
                                                    // backend showcase rather than only in tests.
                                                    Button::new("Disabled")
                                                        .variant(ButtonVariant::Secondary)
                                                        .disabled(true)
                                                )
                                        )
                                )
                                .child(
                                    // Text field showcase section (#312) — promoted from
                                    // mkui-wgpu's wgpu-local chrome into the shared contract,
                                    // following the same `Card` promotion pattern (#270/#301,
                                    // ADR 0015). Exercised through every backend's canonical
                                    // showcase (that ADR's step 7), not only each crate's own
                                    // test suite.
                                    Card::new()
                                        .class("p-6")
                                        .child(
                                            View::new()
                                                .class("mb-6")
                                                .child(
                                                    Text::new("Text Field")
                                                        .variant(TextVariant::Heading2)
                                                        .class("text-2xl font-semibold leading-none tracking-tight")
                                                )
                                                .child(
                                                    Text::new("A single-line editable text input, with disabled and invalid states")
                                                        .variant(TextVariant::Caption)
                                                        .class("text-sm text-muted-foreground mt-2")
                                                )
                                        )
                                        .child(
                                            View::new()
                                                .class("flex flex-col gap-4")
                                                .child(TextField::new("").placeholder("Email"))
                                                .child(
                                                    TextField::new("disabled@example.com")
                                                        .disabled(true)
                                                )
                                                .child(
                                                    TextField::new("not-an-email")
                                                        .placeholder("Email")
                                                        .invalid(true)
                                                )
                                        )
                                )
                                .child(
                                    // Image showcase section (#196) — the second
                                    // decoded-bytes/GPU-texture component promoted
                                    // through every backend per ADR 0015's
                                    // checklist. `Image::from_encoded` cannot fail
                                    // on this hardcoded, known-good checkerboard,
                                    // so `.expect(...)` here documents that rather
                                    // than hiding a real fallibility concern.
                                    Card::new()
                                        .class("p-6")
                                        .child(
                                            View::new()
                                                .class("mb-6")
                                                .child(
                                                    Text::new("Image Component")
                                                        .variant(TextVariant::Heading2)
                                                        .class("text-2xl font-semibold leading-none tracking-tight")
                                                )
                                                .child(
                                                    Text::new("Decoded-bytes source, contain vs. cover fit, rounded clip")
                                                        .variant(TextVariant::Caption)
                                                        .class("text-sm text-muted-foreground mt-2")
                                                )
                                        )
                                        .child(
                                            // Natural size: the 8x8 checkerboard at its own
                                            // intrinsic dimensions — no `flex-1`, so the box
                                            // matches the source exactly and neither fit mode
                                            // has anything to do.
                                            View::new()
                                                .class("flex mb-4")
                                                .child(
                                                    Image::from_encoded(CHECKERBOARD_PNG)
                                                        .expect("hardcoded checkerboard PNG always decodes")
                                                )
                                        )
                                        .child(
                                            // `flex-1` grows the box to the row's full width —
                                            // mkui-layout's only per-leaf sizing lever (there is
                                            // no standalone `w-*`/`h-*` leaf override) — while
                                            // height stays the image's own intrinsic 8px, so the
                                            // box ends up far wider than tall relative to the
                                            // square source. Contain scales the image down to
                                            // fit *within* that box (here: it already fits both
                                            // axes, so it stays native-sized) and letterboxes
                                            // the rest transparent either side, rather than
                                            // cropping or stretching.
                                            View::new()
                                                .class("flex mb-4")
                                                .child(
                                                    Image::from_encoded(CHECKERBOARD_PNG)
                                                        .expect("hardcoded checkerboard PNG always decodes")
                                                        .class("flex-1")
                                                        .fit(ImageFit::Contain)
                                                )
                                        )
                                        .child(
                                            // Same grown, mismatched box, same source — Cover
                                            // instead scales the image *up* until it fully
                                            // covers the box, cropping the overflow instead of
                                            // letterboxing it.
                                            View::new()
                                                .class("flex")
                                                .child(
                                                    Image::from_encoded(CHECKERBOARD_PNG)
                                                        .expect("hardcoded checkerboard PNG always decodes")
                                                        .class("flex-1")
                                                        .fit(ImageFit::Cover)
                                                )
                                        )
                                )
                        )
                )
                .child(
                    // Footer
                    View::new()
                        .class("border-t mt-auto")
                        .child(
                            View::new()
                                .class("container mx-auto px-4 py-6")
                                .child(
                                    View::new()
                                        .class("flex flex-col sm:flex-row items-center justify-between gap-4 text-sm text-muted-foreground")
                                        .child(
                                            Text::new("Built with 🦀 Rust")
                                                .variant(TextVariant::Caption)
                                        )
                                        .child(
                                            View::new()
                                                .class("flex items-center gap-4")
                                                .child(
                                                    Button::new("Source Code")
                                                        .class("hover:text-foreground transition-colors p-0 h-auto")
                                                        .variant(ButtonVariant::Link)
                                                        .on_press(|| {
                                                            #[cfg(feature = "web")]
                                                            {
                                                                web_sys::window()
                                                                    .unwrap()
                                                                    .open_with_url("https://github.com/mikbry/ui")
                                                                    .ok();
                                                            }
                                                            #[cfg(feature = "console")]
                                                            {
                                                                println!("Source code link clicked!");
                                                            }
                                                            #[cfg(feature = "wgpu")]
                                                            {
                                                                println!("Source code link clicked!");
                                                            }
                                                        })
                                                )
                                                .child(Text::new("•").variant(TextVariant::Caption))
                                                .child(
                                                    Text::new("MIT/Apache Licensed")
                                                        .variant(TextVariant::Caption)
                                                )
                                        )
                                )
                        )
                )
        ))
}

// -------------------------------------------------------------------------
// Component gallery (#310) — native counterpart to #278's web/shadcn
// parity fixture, with no external reference to compare against.
// -------------------------------------------------------------------------

/// Component gallery UI — a plain catalogue of every component currently
/// promoted into the shared `mkui-core` contract (today: [`Button`] and
/// [`Card`]), one section per component with its meaningful state variants
/// shown side by side. Unlike [`create_showcase_ui`]'s marketing-style
/// landing page, this tree carries no branding content — its only job is
/// to keep every contract-level component visible and inspectable on the
/// native backend.
///
/// As #270's promotion programme lifts more components into the shared
/// contract, growing this gallery's coverage is the expected follow-up in
/// each promotion PR — add that component's section here rather than
/// letting the gallery go stale the way the landing-page showcase did
/// before #310.
pub fn create_gallery_ui() -> Result<Mkui, MkuiError> {
    Ok(Mkui::new()?.child(
        View::new().class("flex-1").child(gallery_header()).child(
            View::new()
                .class("container mx-auto py-8 px-4 max-w-4xl space-y-8")
                .child(button_section())
                .child(card_section()),
        ),
    ))
}

fn gallery_header() -> View {
    View::new().class("border-b").child(
        View::new()
            .class("container mx-auto px-4 h-16 flex items-center")
            .child(
                View::new()
                    .class("flex items-center space-x-4")
                    .child(
                        Text::new("mkui Component Gallery")
                            .variant(TextVariant::Heading2)
                            .class("text-xl font-semibold"),
                    )
                    .child(
                        Text::new("Every shared mkui-core contract component, one section each")
                            .variant(TextVariant::Caption)
                            .class("text-sm text-muted-foreground mt-2"),
                    ),
            ),
    )
}

fn section_heading(name: &str, variants: &str) -> View {
    View::new()
        .class("mb-6")
        .child(
            Text::new(name)
                .variant(TextVariant::Heading2)
                .class("text-2xl font-semibold leading-none tracking-tight"),
        )
        .child(
            Text::new(variants)
                .variant(TextVariant::Caption)
                .class("text-sm text-muted-foreground mt-2"),
        )
}

/// Every `ButtonVariant` plus a disabled state, side by side — the gallery's
/// Button section.
fn button_section() -> Card {
    Card::new()
        .class("p-6")
        .child(section_heading(
            "Button",
            "primary · secondary · destructive · outline · ghost · link · disabled",
        ))
        .child(
            View::new()
                .class("flex flex-wrap gap-4")
                .child(Button::new("Primary").variant(ButtonVariant::Primary))
                .child(Button::new("Secondary").variant(ButtonVariant::Secondary))
                .child(Button::new("Destructive").variant(ButtonVariant::Destructive))
                .child(Button::new("Outline").variant(ButtonVariant::Outline))
                .child(Button::new("Ghost").variant(ButtonVariant::Ghost))
                .child(Button::new("Link").variant(ButtonVariant::Link))
                .child(
                    Button::new("Disabled")
                        .variant(ButtonVariant::Secondary)
                        .disabled(true),
                ),
        )
}

/// A default `Card` next to one carrying a footer action row — the
/// gallery's Card section.
fn card_section() -> View {
    View::new()
        .class("space-y-4")
        .child(section_heading("Card", "default · with actions"))
        .child(
            View::new()
                .class("flex flex-wrap gap-4")
                .child(
                    Card::new()
                        .class("p-6")
                        .child(
                            Text::new("Default")
                                .variant(TextVariant::Heading2)
                                .class("text-xl font-semibold"),
                        )
                        .child(
                            Text::new("A plain card with no actions.")
                                .variant(TextVariant::Caption)
                                .class("text-sm text-muted-foreground mt-2"),
                        ),
                )
                .child(
                    Card::new()
                        .class("p-6")
                        .child(
                            Text::new("With actions")
                                .variant(TextVariant::Heading2)
                                .class("text-xl font-semibold"),
                        )
                        .child(
                            Text::new("A card whose footer carries action buttons.")
                                .variant(TextVariant::Caption)
                                .class("text-sm text-muted-foreground mt-2 mb-4"),
                        )
                        .child(
                            View::new()
                                .class("flex gap-2")
                                .child(Button::new("Cancel").variant(ButtonVariant::Outline))
                                .child(Button::new("Confirm").variant(ButtonVariant::Primary)),
                        ),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::Mkui as CoreMkui;
    use mkui_runtime::NodeKind;

    /// #310 — the Button section must render all six `ButtonVariant`s
    /// (primary/secondary/destructive/outline/ghost/link) plus a disabled
    /// state, side by side, inside a `Card`. Lowers into a backend-free
    /// `mkui_core::components::Mkui` tree (no `wgpu`/`web`/`console`
    /// feature required) so this runs under the default `--workspace` test
    /// leg, the same way `mkui-core`'s own `components.rs` tests do.
    #[test]
    fn button_section_renders_all_six_variants_plus_a_disabled_state() {
        let app = CoreMkui::new().child(button_section());
        let root = app.tree().get(app.tree().root()).unwrap();
        let card = app.tree().get(root.children[0]).unwrap();
        assert!(matches!(card.kind, NodeKind::Card(_)));
        assert_eq!(card.children.len(), 2, "heading + button row");

        let row = app.tree().get(card.children[1]).unwrap();
        assert_eq!(
            row.children.len(),
            7,
            "primary/secondary/destructive/outline/ghost/link/disabled"
        );
        let mut disabled_count = 0;
        for &child_id in &row.children {
            let node = app.tree().get(child_id).unwrap();
            let NodeKind::Button(button) = &node.kind else {
                panic!("expected every button-row child to be a Button");
            };
            if button.disabled {
                disabled_count += 1;
            }
        }
        assert_eq!(disabled_count, 1, "exactly one disabled button variant");
    }

    /// #310 — the Card section must render a default card next to one
    /// carrying a footer action row, side by side.
    #[test]
    fn card_section_renders_a_default_card_and_a_card_with_action_buttons() {
        let app = CoreMkui::new().child(card_section());
        let root = app.tree().get(app.tree().root()).unwrap();
        let section = app.tree().get(root.children[0]).unwrap();
        assert_eq!(section.children.len(), 2, "heading + card row");

        let row = app.tree().get(section.children[1]).unwrap();
        assert_eq!(row.children.len(), 2, "default card + with-actions card");
        for &card_id in &row.children {
            let node = app.tree().get(card_id).unwrap();
            assert!(matches!(node.kind, NodeKind::Card(_)));
        }

        let with_actions = app.tree().get(row.children[1]).unwrap();
        assert_eq!(
            with_actions.children.len(),
            3,
            "heading + description + action-button row"
        );
        let action_row = app.tree().get(with_actions.children[2]).unwrap();
        assert_eq!(action_row.children.len(), 2, "Cancel + Confirm buttons");
    }

    /// #310 acceptance: "note that expectation in the gallery's own doc
    /// comment so it isn't orphaned the way the landing-page showcase was."
    #[test]
    fn gallery_doc_comment_notes_the_expected_follow_up_for_future_promotions() {
        let source = include_str!("lib.rs");
        assert!(
            source.contains("expected follow-up in"),
            "create_gallery_ui's doc comment must note that growing coverage \
             is the expected follow-up as #270 promotes more components"
        );
    }

    /// Issue #94 — every one of the seven `showcase-common` button actions
    /// must carry a wgpu-gated cfg arm alongside its web/console arms, so
    /// native WGPU interaction is complete. This source-level assertion is
    /// backend-agnostic: it inspects the source text directly rather than
    /// requiring the `wgpu` feature to be active to compile a runtime check.
    /// (The needle below is assembled at runtime so this comment and the
    /// search literal cannot themselves be miscounted as button arms.)
    #[test]
    fn all_seven_actions_have_a_wgpu_arm() {
        let source = include_str!("lib.rs");
        // Assemble the needle so neither this line nor the doc comment above
        // can ever be miscounted as one of the seven button arms.
        let needle = format!("#[cfg(feature = {q}wgpu{q})]", q = '"');
        let wgpu_arms = source.matches(&needle).count();
        assert_eq!(
            wgpu_arms, 7,
            "expected a wgpu cfg arm on each of the seven button actions, found {wgpu_arms}"
        );
    }
}
