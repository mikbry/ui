# ADR 0010 — `PlatformHost` trait for clipboard and native share

## Status

Accepted (Sprint 8, 2026-11-10).

## Context

Issue #173 (wgpu host-embedding) tracks sandboxed deployment targets where
mkui is embedded inside a third-party `wgpu` game host — fonts, icons, and
textures already must load only from host-supplied bytes, with no arbitrary
filesystem reads and no network requests (#173's hosting constraints). Issue
#206 extends that boundary to two more OS capabilities a UI inevitably wants:
copying text to the clipboard ("copy link") and invoking the OS-native share
sheet ("share").

`mkui-core` is the domain-free contract crate (ADR 0001): it depends on
`thiserror` and `mkui-text`/`mkui-runtime` only, never on a backend-facing
crate such as `web-sys` or a platform clipboard library. Clipboard and share
APIs are exactly that kind of backend-facing dependency, so `mkui-core`
cannot call them directly — and, per #206, must not even once a sandboxed
target exists.

Separately, `mkui-runtime`'s `ActionRegistry`/`RuntimeCtx` already draws a
line for actions: "Actions mark the tree dirty and emit `RuntimeSignal`s;
they do **not** reach for renderer APIs directly" (ADR 0005). Clipboard/share
calls are not renderer concerns and do not need a redraw signal, so routing
them through `RuntimeSignal` would overload a mechanism designed for a
different purpose (and `mkui-runtime` sits one layer *below* `mkui-core`;
it would be the wrong crate to own an OS-facing contract).

## Decision

Add `mkui_core::platform::PlatformHost`, a plain trait with no mkui-owned
plumbing:

```rust
pub trait PlatformHost {
    fn copy_to_clipboard(&self, text: &str) -> Result<(), MkuiError>;
    fn share(&self, request: ShareRequest) -> Result<(), MkuiError>;
}
```

`mkui-core` ships exactly one implementation, `NoopPlatformHost` — a
zero-dependency default that performs no OS work and always succeeds. Every
real implementation (`web_sys::Clipboard`/`navigator.share`, `NSPasteboard`/
`UIActivityViewController`, a desktop clipboard crate, …) lives in
host/application code, never in `mkui-core` or a backend crate.

Wiring requires no new action-registry plumbing. `Button::on_press` already
accepts an arbitrary `Fn()` closure supplied by application code; the host
wires platform services by capturing a `Rc<dyn PlatformHost>` inside that
closure and calling it when the action fires:

```ignore
let host: Rc<dyn PlatformHost> = Rc::new(my_apps::WebPlatformHost::new());
Button::new("Copy link").on_press(move || {
    let _ = host.copy_to_clipboard("https://example.com");
});
```

mkui-core's own code never constructs or calls a concrete `PlatformHost` —
it only defines the trait and `NoopPlatformHost`. The OS call happens inside
application-authored code, which satisfies #206's acceptance criterion that
a UI action round-trip through a host-provided implementation with no direct
OS API call from inside mkui.

## Consequences

- `mkui-core` gains a new public module (`platform`) and two new prelude
  exports (`PlatformHost`, `ShareRequest`) plus `NoopPlatformHost`. No new
  crate dependency, no new `Cargo.toml` feature.
- Backends (`mkui-web`, `mkui-wgpu`, `mkui-console`) are free to ship their
  own `PlatformHost` implementation as a convenience (e.g. `mkui-web` could
  offer a `WebPlatformHost` built on `web_sys`), but shipping one is out of
  scope for #206 — the trait and the no-op default are the load-bearing
  minimum the issue asks for. A follow-up issue can add backend-provided
  implementations once a concrete consumer needs one (tracked as #220).
- Unit tests exercise the full round-trip (a `Button::on_press` closure
  firing through `ActionRegistry` into a recording `PlatformHost` test
  double) entirely inside `mkui-core`, with no OS, filesystem, or network
  calls — see `crates/mkui-core/src/platform.rs`.

## Alternatives considered

- **Thread a `PlatformHost` handle through `RuntimeCtx`/`ActionRegistry`
  (mirroring `RuntimeSignal`).** Rejected: `mkui-runtime` sits below
  `mkui-core` in the dependency graph and is explicitly scoped to
  tree/action/class-parser storage, not an OS-facing contract; and
  `RuntimeCtx`'s existing signal mechanism is designed for renderer
  notifications (redraw), a different concern than an app-authored action
  calling out to a host service it already has a handle to. Closure capture
  is strictly simpler and needs zero new runtime plumbing.
- **Ship a concrete `PlatformHost` per backend now** (e.g. `web_sys` in
  `mkui-web`). Rejected as surplus scope for #206 — the issue's acceptance
  criteria ask only for the trait, a no-op/test double, and the no-fs/no-net
  guarantee. Deferred to a follow-up issue (#220).
