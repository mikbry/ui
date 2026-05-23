# ADR 0003 — `mkui-web` registry-based extension

## Status

Accepted (Sprint 1, 2026-05-20).

## Context

Before [#14](https://github.com/mikbry/ui/issues/14), `mkui-web` rendered
components by downcasting `dyn Component` to a fixed, closed set of concrete
types: `View`, `Text`, `Button`, `ThemeSelector`. Unknown components — anything
a downstream crate might define — silently rendered an `Unsupported Component`
placeholder.

This had two failure modes:

1. Downstream consumers who defined their own components could not render them
   on the web backend without forking `mkui-web` and adding a new arm to the
   downcast cascade.
2. The "unsupported" placeholder was easy to miss in review and easy to ship to
   production by accident — a silent fallback rather than a loud failure.

The web backend is the first of multiple backends ([ADR 0001](0001-mkui-core-as-contract-crate.md))
that face this pattern. A solution that scales to `mkui-console` and
`mkui-wgpu` was preferable to a per-backend escape hatch.

## Decision

Replace the closed-set downcast with `WebRendererRegistry` — a `TypeId`-keyed
dispatch table.

- Downstream crates register custom component render functions via
  `registry.register::<MyComponent>(render_fn)`.
- The renderer consults the registry on each component encounter and dispatches
  to the registered function.
- Unknown components fail loudly in debug paths or expose a deliberate fallback
  hook — there is no silent "unsupported" placeholder.
- The built-in component set (`View`, `Text`, `Button`, `ThemeSelector`) is
  registered at backend init using the same mechanism — there is no special
  case for built-ins.

The pattern is documented and exercised by an integration smoke test at
`crates/mkui-web/tests/custom_component_extension.rs`.

## Consequences

- `mkui-web` no longer needs editing to support new component types.
  Downstream consumers extend the renderer from the outside.
- The backend rendering boundary is documented, testable, and consistent
  across backends — the same registry pattern is the recommended template for
  `mkui-console` and `mkui-wgpu` as they grow custom-component support.
- Registry lookup is a `HashMap<TypeId, _>` hit on each component render. The
  cost is bounded and amortised by the component tree's overall render cost;
  no measurable regression vs. the closed-set downcast cascade.
- The registry is mutable at app-init time and frozen after, which keeps
  thread-safety reasoning simple — no interior mutability needed during render.

## Alternatives considered

- **Trait-based dispatch** (`trait Renderable { fn render(&self, ctx); }` on
  every component). Rejected. Requiring every component type to know about
  backend rendering details would break the contract-crate boundary
  ([ADR 0001](0001-mkui-core-as-contract-crate.md)) — `mkui-core` would need
  to depend on `web-sys`, `wasm-bindgen`, or some backend-rendering trait
  package, and the same component would need a separate impl for every
  backend it might ever be rendered on. That coupling is exactly what
  `mkui-core` was created to avoid.
- **Codegen / macro-based dispatch** (e.g. proc-macro that enumerates all
  known components and generates a `match` arm per backend). Rejected. Too
  much machinery for a small dispatch surface. The runtime registry is
  ~50 lines of straightforward code; a macro would be hundreds of lines of
  proc-macro state plus a build-time gate per backend.
