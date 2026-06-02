//! Scene-emitting UI components for mkui-wgpu.
//! Components are small free functions that append primitives to a `Scene`.
//!
//! The surface mirrors [`miklabs/ui`] (`mkui-core::components`) and
//! [shadcn/ui]: every view is composed from `Card`, `Button`, `TextField`,
//! `Slider`, `ChipGroup`, `Scrollbar`, `Swatch`, `Label`, `Text`. Each
//! component takes a `variant` and a `size` and resolves concrete colors
//! through [`WgpuTheme`](crate::theme::WgpuTheme) — the same cva-style shape
//! shadcn uses for its `ButtonVariant` / `ButtonSize`. State
//! (`ButtonState::Idle` / `Active`) is orthogonal to variant and supplied
//! per-frame, because the underlying renderer is immediate-mode and does not
//! retain a component tree between frames.
//!
//! Each component is a free function that mutates a `&mut Scene`, consuming a
//! [`WgpuTheme`](crate::theme::WgpuTheme) for style resolution. Components
//! compose: pass the result of one into another (e.g. [`panel`] returns the
//! inner rect for child placement). Related functions are grouped per file:
//!
//! - **primitives** — [`panel`] / [`titled_panel`], [`label`], [`button_with`].
//!   These take pre-resolved `PanelStyle` / `ButtonStyle` values and are the
//!   escape hatch when a caller needs a one-off style that isn't in the theme.
//! - **variant-driven widgets** — [`card`], [`button`], [`text_field`],
//!   [`slider`], [`chip_group`], [`scrollbar`], [`swatch`], [`heading`],
//!   [`text`], [`info_list`]. Each takes a variant / size / state and pulls concrete
//!   colors from the theme. This is the layer view code calls.
//! - **atoms** — [`badge`], [`dot`]. Self-contained shadcn-aligned signals.
//!
//! Built-in components registered with `WgpuRendererRegistry` (see `bridge.rs`)
//! cover the shadcn-aligned atoms; custom components register via
//! `WgpuRenderable::render` (see `bridge::WgpuRenderable`).
//!
//! [shadcn/ui]: https://ui.shadcn.com/
//! [`miklabs/ui`]: https://github.com/mikbry/ui

mod badge;
mod button;
mod card;
mod chip_group;
mod dot;
mod info_list;
mod panel;
mod scrollbar;
mod slider;
mod swatch;
mod text;
mod text_field;

pub use badge::badge;
pub use button::{button, button_with};
pub use card::card;
pub use chip_group::chip_group;
pub use dot::dot;
pub use info_list::info_list;
pub use panel::{panel, titled_panel};
pub use scrollbar::scrollbar;
pub use slider::{slider, SliderRegions};
pub use swatch::swatch;
pub use text::{heading, label, text};
pub use text_field::text_field;
