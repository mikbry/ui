//! Native component gallery (#310) — drives
//! `examples/showcase-common::create_gallery_ui` through the mkui-wgpu
//! declarative bridge, same shape as `examples/native-showcase`'s
//! `create_showcase_ui` consumer.
//!
//! This is the native (`mkui-wgpu`) counterpart to #278's web/shadcn
//! parity fixture: #278 compares a component against an external
//! reference, this example has none — its only job is to make every
//! component with a shared `mkui-core` contract representation visible
//! and inspectable on the native backend as #270's promotion programme
//! adds more of them.
//!
//! `HEADLESS=1 cargo run -p component-gallery --release` exits after a
//! single walker pass — useful for CI smoke validation when there is no
//! display server, matching `native-showcase`'s own `HEADLESS` contract.

use showcase_common::create_gallery_ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    mkui::run!(create_gallery_ui, wgpu)
}
