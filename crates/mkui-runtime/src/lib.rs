#![forbid(unsafe_code)]
//! # mkui-runtime — portable application-tree substrate
//!
//! `mkui-runtime` is the *contract-implementation* layer beneath every mkui
//! binding (Rust, web, console, C, Python). It owns the concrete data
//! structures the bindings agree on:
//!
//! - [`AppTree`] — arena-backed scene graph indexed by [`NodeId`].
//! - [`Node`] / [`NodeKind`] — the runtime form of `View` / `Text` / `Button`
//!   plus a [`NodeKind::Custom`] slot the extension registry plugs into.
//! - [`ActionRegistry`] / [`ActionId`] — single-threaded callback table.
//!   Action closures are stored once, referenced by id everywhere else
//!   (so C and Python can register callbacks across the FFI boundary
//!   without smuggling Rust closures through it).
//! - [`StyleClass`] — utility-class strings and the [`ResolvedStyle`] they
//!   parse to.
//! - JSON snapshots (feature `snapshot`) — canonical, key-sorted
//!   serializations used by the parity tests to prove Rust, C, and Python
//!   constructions all produce the same tree.
//!
//! ## Why a separate crate
//!
//! See ADR 0005 (`docs/architecture/0005-mkui-runtime-portable-substrate.md`).
//! Short version: `mkui-core` remains a pure-contract crate (per ADR 0001).
//! Storage, action plumbing, and class parsing are *implementation* details —
//! they belong in a crate every binding can depend on without dragging the
//! Component trait machinery along. Crucially the runtime types are
//! `repr(Rust)` value types with no backend dependencies (`grep` verified:
//! zero `wgpu::*`, `web_sys::*`, `crossterm::*`).
//!
//! ## Single-threaded by default
//!
//! [`ActionRegistry`] is intentionally *not* `Send + Sync`. Today mkui has no
//! multi-threaded runtime; making the registry thread-safe would force every
//! binding to thread `Send + Sync` bounds through closures that never cross
//! threads. The bound can be added later when a real concurrent runtime
//! arrives.

pub mod actions;
pub mod props;
pub mod style;
pub mod tree;

#[cfg(feature = "snapshot")]
pub mod snapshot;

pub use actions::{ActionId, ActionRegistry, LocalAction, RuntimeCtx, RuntimeSignal};
pub use props::{ButtonVariant, ButtonVariantParseError, TextVariant, TextVariantParseError};
pub use style::{ClassParseError, ResolvedStyle, StyleClass};
pub use tree::{AppTree, ButtonProps, Node, NodeId, NodeKind, TextProps, ViewProps};
