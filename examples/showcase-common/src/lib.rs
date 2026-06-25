use mkui::prelude::*;
use mkui_core::components::{Button, Text, View};
use mkui_core::headless::{ButtonVariant, TextVariant};

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
                                    // Button showcase section
                                    View::new()
                                        .class("rounded-lg border bg-card text-card-foreground shadow-sm p-6")
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

#[cfg(test)]
mod tests {
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
