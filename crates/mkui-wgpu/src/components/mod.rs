//! Scene-emitting UI components for mkui-wgpu.
//! Components are small free functions that append primitives to a `Scene`.
//!
//! The surface mirrors [`miklabs/ui`] (`mkui-core::components`) and
//! [shadcn/ui]: every view is composed from `Card`, `Button`, `TextField`,
//! `Slider`, `Toggle`, `ChipGroup`, `SegmentedSelect`, `Scrollbar`, `Swatch`,
//! `Label`, `Text`. Each component takes a `variant` and a `size` and
//! resolves concrete colors through [`WgpuTheme`](crate::theme::WgpuTheme) —
//! the same cva-style shape shadcn uses for its `ButtonVariant` /
//! `ButtonSize`. State (`ButtonState::Idle` / `Active`) is orthogonal to
//! variant and supplied per-frame, because the underlying renderer is
//! immediate-mode and does not retain a component tree between frames.
//! `button`/`button_with`'s `disabled` / `focused` flags (#186) follow the
//! same per-frame, caller-supplied contract.
//!
//! Each component is a free function that mutates a `&mut Scene`, consuming a
//! [`WgpuTheme`](crate::theme::WgpuTheme) for style resolution. Components
//! compose: pass the result of one into another (e.g. [`panel`] returns the
//! inner rect for child placement). Related functions are grouped per file:
//!
//! - **primitives** — [`panel`] / [`titled_panel`] / [`glass_panel`], [`label`],
//!   [`button_with`].
//!   These take pre-resolved `PanelStyle` / `ButtonStyle` values and are the
//!   escape hatch when a caller needs a one-off style that isn't in the theme.
//! - **variant-driven widgets** — [`card`], [`button`], [`text_field`],
//!   [`text_field_editable`], [`slider`], [`slider_with_label`], [`toggle`],
//!   [`progress_bar`], [`chip_group`], [`segmented_select`], [`scrollbar`],
//!   [`scroll_area`], [`swatch`], [`heading`], [`text`], [`info_list`],
//!   [`toast_stack`], [`toast_history`], [`dialog`], [`titled_dialog`].
//!   Each takes a variant / size / state and pulls concrete colors from
//!   the theme. This is the layer view code calls.
//! - **atoms** — [`badge`], [`dot`], [`counter_chip`]. Self-contained
//!   shadcn-aligned signals.
//!
//! Built-in components registered with `WgpuRendererRegistry` (see `bridge.rs`)
//! cover the shadcn-aligned atoms; custom components register via
//! `WgpuRenderable::render` (see `bridge::WgpuRenderable`).
//!
//! ## Keyboard/gamepad operability (#190)
//!
//! [`toggle`], [`slider`]/[`slider_with_label`], and [`chip_group`]/
//! [`segmented_select`] are pure render functions — they take whatever
//! `checked`/`fraction`/`selected` the caller holds this frame and draw it,
//! same as every other widget here. The actual keyboard/gamepad-driven state
//! transitions live in [`mkui_core::headless`] (`Toggle`, `Slider`,
//! `SegmentedSelect`): identical logic across every backend, which a future
//! gamepad-button mapping (#203) also calls into. See each module's doc
//! comment for the paired headless type.
//!
//! [shadcn/ui]: https://ui.shadcn.com/
//! [`miklabs/ui`]: https://github.com/mikbry/ui

mod badge;
mod button;
mod card;
mod chip_group;
mod counter_chip;
mod dialog;
mod dot;
mod info_list;
mod panel;
mod progress_bar;
mod scroll_area;
mod scrollbar;
mod segmented_select;
mod slider;
mod swatch;
mod text;
mod text_field;
mod toast;
mod toggle;

pub use badge::badge;
pub use button::{button, button_with, ButtonRegions, MIN_TOUCH_TARGET_PX};
pub use card::card;
pub use chip_group::chip_group;
pub use counter_chip::{counter_chip, CounterChipRegions};
pub use dialog::{dialog, titled_dialog};
pub use dot::dot;
pub use info_list::info_list;
pub use panel::{glass_panel, panel, titled_panel};
pub use progress_bar::progress_bar;
pub use scroll_area::{scroll_area, ScrollAreaRegions};
pub use scrollbar::scrollbar;
pub use segmented_select::segmented_select;
pub use slider::{slider, slider_with_label, SliderRegions};
pub use swatch::swatch;
pub use text::{heading, label, text};
pub use text_field::{text_field, text_field_editable};
pub use toast::{toast_history, toast_stack};
pub use toggle::{toggle, ToggleRegions};
