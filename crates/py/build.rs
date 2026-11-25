//! Configures linkage against the local Python interpreter for the
//! `cdylib` build path. PyO3's `extension-module` feature deliberately
//! skips emitting `-l python3.x` because, in the normal flow, the
//! resulting extension is loaded *by* Python which provides the symbols.
//!
//! On macOS the linker enforces this contract more strictly than on
//! Linux: the cdylib fails to link without an explicit
//! `-undefined dynamic_lookup`, which `pyo3_build_config::add_extension_module_link_args`
//! emits. Without this build script (which the Sprint 4 round-7 PR
//! omitted), `cargo build -p mkui-py` fails on macOS even though it
//! works on the Ubuntu CI image.

fn main() {
    pyo3_build_config::add_extension_module_link_args();
}
