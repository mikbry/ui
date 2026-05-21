# Miky catalog → shadcn/ui mapping

> **shadcn/ui is the naming + implementation target for mkui.** StoneSketch's `stonesketch-gui` conforms to shadcn loosely (per `astoneer/stonesketch/docs/gui.md` lines 130-178: *"components — scene builders mirroring mkui-core::components and shadcn's widget catalog"*), but where StoneSketch and shadcn diverge, **prefer shadcn**. Miky's `DESIGN.md` (`mikbry/miky-internal/DESIGN.md`) names ~30 components the Operator Console requires. This document maps each Miky-named component to its shadcn/ui equivalent, lists the variants/sizes/states, and flags compositions where Miky's component is a *composition* of multiple shadcn primitives rather than a direct mapping.
>
> **Why this exists:** before mkui-wgpu starts shipping atoms, the implementation needs naming + variant + state alignment with shadcn. shadcn names + variants are what downstream consumers (Miky's app, future StoneSketch adoption) will recognize. If we ship a `Chip` that diverges from shadcn's `Badge` in unexpected ways, every consumer pays the migration tax later.
>
> **References:**
> - shadcn/ui canonical catalog: https://ui.shadcn.com/docs/components
> - shadcn variants pattern (cva): https://cva.style
> - Miky catalog source: `/Users/mik/dev/miky-internal/DESIGN.md` lines 518-572 (Components section)
> - StoneSketch's existing mapping: `/Users/mik/dev/astoneer/stonesketch/docs/gui.md` lines 358-372 (the imperative-vs-retained mapping table)
> - `mkui-wgpu`'s existing implementation: `/Users/mik/dev/mikbry/ui/crates/mkui-wgpu/src/{components.rs,theme.rs}` — already exposes shadcn-shaped `ButtonVariant`/`Size`/`State`/`TextVariant` enums

---

## Mapping principles

1. **shadcn's name wins when it's a direct equivalent.** If a Miky-named "Chip" is the same conceptual primitive as shadcn's `Badge`, the mkui crate exposes both names — `mkui_wgpu::components::badge(...)` as the canonical function, `mkui_wgpu::components::chip(...)` as an alias for Miky's usage. The Miky design doc keeps "Chip" in the design language; the code uses shadcn name as primary.

2. **Variants follow shadcn's `cva`-style resolver.** shadcn exposes variants like `default | destructive | outline | secondary | ghost | link` for Button. Miky's "primary / attn / default / ghost" maps onto these. The Rust enum is `ButtonVariant::Default` / `ButtonVariant::Destructive` / etc. — shadcn naming.

3. **Miky-specific variants extend shadcn, never replace.** Where Miky needs a variant shadcn doesn't have (e.g. `attn` for warm-amber attention state), the variant lives alongside shadcn's set, not as a replacement. The shadcn enum gains an `Attn` variant; the cva resolver picks `attn-container` tokens from the theme.

4. **Compositions stay compositions.** Miky's "PRRow" is not a shadcn component. It's a composition: `Card > Row > [Badge(num), Text(title), Badge(repo), TierTag, Badge(ci), Badge(diff), Text(age)]`. The mkui-wgpu API exposes the row builder; the constituent atoms are shadcn-aligned. No new top-level component named `PRRow` ships as a primitive.

5. **Where shadcn doesn't have an equivalent**, the Miky-specific component ships under its Miky name as a first-class component. Examples: `StatePill` (9-state agent encoding), `LedgerChip`, `Kbd`, `TierTag`, `RoleBadge`. These are genuinely application-specific and have no canonical shadcn name.

---

## Atoms

### Chip → shadcn `Badge`

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `Chip` | `Badge` | `mkui_wgpu::components::badge` (primary); `chip` (alias) |
| **Geometry** | 22px tall, full-rounded | `h-6`, `rounded-full` | Both |
| **Variants** | default, attn, ok, warn, danger, accent, mono, bare | default, destructive, outline, secondary | `BadgeVariant::{Default, Destructive, Outline, Secondary, Attn, Ok, Warn, Accent, Mono, Bare}` — shadcn variants kept verbatim + Miky-specific ones added |
| **States** | — | — | None (badges are non-interactive) |
| **Notes** | Miky's `attn` chip maps to shadcn `destructive` *visually* but semantically different (attention != error). Keep both. |

### Dot → no shadcn equivalent

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `Dot` | (none — shadcn has no status-dot primitive) | `mkui_wgpu::components::dot` |
| **Geometry** | 6–8px circle | — | — |
| **Variants** | ok, warn, danger, attn (with 3px halo) | — | `DotVariant::{Ok, Warn, Danger, Attn, Neutral}` |
| **Notes** | shadcn doesn't ship a status-dot. This is a Miky-named first-class atom in mkui. |

### StatePill → shadcn `Badge` (composition)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `StatePill` | composition of `Badge` + animated `Dot` | `mkui_wgpu::components::state_pill` |
| **Geometry** | 22px, full-rounded | — | — |
| **Variants** | 9 states (running, needs-you, errored, review-pending, review-in-flight, verdict-captured, applying-verdict, merge-pending, completed) | — | `StatePillState::{Running, NeedsYou, Errored, ReviewPending, ReviewInFlight, VerdictCaptured, ApplyingVerdict, MergePending, Completed}` |
| **Animation** | `running` + `review-in-flight` animate the pip via `pulse 2.4s` | — | Uses the existing motion contract |
| **Notes** | Miky-specific. shadcn has nothing like this. First-class component in mkui. |

### RoleBadge → shadcn `Badge` (variant)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `RoleBadge` | `Badge` with `BadgeVariant::Role(RepoRole)` | `mkui_wgpu::components::role_badge` (thin wrapper); `Badge { variant: Role(...) }` (canonical) |
| **Geometry** | 14px tall, label-caps text | — | Smaller than default Badge |
| **Variants** | MAIN (accent), COMPANION (text-tertiary), INFRA (purple), SIBLING (text-quaternary) | — | `RepoRole::{Main, Companion, Infra, Sibling}` |
| **Notes** | Could be folded into `Badge` with a `size: BadgeSize::Caps` variant + a `role: RepoRole` parameter. The wrapper exists for Miky's call-site terseness. |

### TierTag → shadcn `Badge` (variant)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `TierTag` | `Badge` with tier-specific color tokens | `mkui_wgpu::components::tier_tag` (wrapper); `Badge { variant: Tier(...) }` (canonical) |
| **Geometry** | 14px tall, label-caps | — | — |
| **Variants** | 5 tiers (mechanical, template, typed-primitive, substrate, novel-surface) | — | `CodexTier::{Mechanical, Template, TypedPrimitive, Substrate, NovelSurface}` |
| **Notes** | Miky-specific (Codex review tier classifier). Wraps `Badge` with codex-tier color tokens. |

### LedgerChip → shadcn `Badge` (variant)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `LedgerChip` | `Badge` with a leading icon slot | `mkui_wgpu::components::ledger_chip` |
| **Geometry** | 17px tall, purple, with grid icon | — | — |
| **Notes** | Miky-specific. First-class for now; could be folded into `Badge` with `leading_icon: Option<Icon>` if 3+ similar use cases appear. |

### Kbd → no shadcn equivalent (kind of)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `Kbd` | shadcn has no `Kbd` component, but the convention is to use `<kbd>` HTML in docs. shadcn's command palette example uses a similar visual. | `mkui_wgpu::components::kbd` |
| **Geometry** | 9–10px mono, faint background, 0.5px border | — | — |
| **Notes** | Standard HTML `<kbd>` semantic; the Miky name matches. First-class atom in mkui — name unchanged. |

### Avatar → shadcn `Avatar`

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `Avatar` | `Avatar` | `mkui_wgpu::components::avatar` ✓ direct mapping |
| **Geometry** | Circular gradient (245°→280° hue ramp) | shadcn supports image + fallback | — |
| **Variants** | Miky only uses gradient (no image) | shadcn supports image, image+fallback, fallback-only | `AvatarSource::{Image(url), Gradient(hue_a, hue_b), Initials(text)}` |
| **Notes** | shadcn's API is richer. mkui exposes the shadcn API; Miky uses only the gradient variant. |

---

## Inputs

### Button → shadcn `Button`

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `Button` | `Button` ✓ | `mkui_wgpu::components::button` (already exists) |
| **Variants** | primary, attn, default, ghost, sm-default, sm-ghost | default, destructive, outline, secondary, ghost, link | `ButtonVariant::{Default, Destructive, Outline, Secondary, Ghost, Link, Attn}` — shadcn 6 + Miky's `Attn` (the warm-amber attention state has no shadcn equivalent) |
| **Sizes** | sm (22px) + default (26px) | default, sm, lg, icon | `ButtonSize::{Default, Sm, Lg, Icon}` — shadcn 4, Miky uses Default + Sm |
| **States** | idle, hover, active | idle, hover, active, disabled | `ButtonState::{Idle, Active}` (already exists in mkui-wgpu — extend with `Disabled` if needed) |
| **Notes** | mkui-wgpu already has shadcn-aligned variants/sizes/states. Add `Attn` variant. Map Miky's "primary" → `ButtonVariant::Default`, "ghost" → `ButtonVariant::Ghost`, "attn" → `ButtonVariant::Attn`. |

### SearchInput → shadcn `Input` (composition)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `SearchInput` | `Input` with a leading magnifier icon + trailing `Kbd` | `mkui_wgpu::components::search_input` (wrapper); `Input` (canonical) |
| **Geometry** | 240px, 26px tall, full-rounded | shadcn: `h-9`, `rounded-md` (shadcn doesn't full-round inputs by default) | — |
| **Variants** | One variant (titlebar) | default, with icon, with addon | `InputVariant::{Default, Search, ...}` |
| **States** | idle, focus (accent border + 3px glow) | idle, focus, disabled, invalid | All four |
| **Notes** | Miky's `SearchInput` is a composition: `Input` + leading `Icon` + trailing `Kbd`. mkui can either ship `SearchInput` as a wrapper or expose `Input` with slot APIs. **Prefer slot APIs** (matches shadcn's actual implementation, more reusable). |

### SegmentedControl → shadcn `Tabs` or `ToggleGroup`

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `SegmentedControl` | shadcn's `Tabs` with `variant="segmented"`, or `ToggleGroup` with `type="single"` | `mkui_wgpu::components::segmented_control` (alias); canonical = `ToggleGroup` |
| **Geometry** | Pill track with sliding thumb | shadcn doesn't ship a sliding thumb — the segmented look is achieved via background swap | — |
| **Notes** | Map to shadcn `ToggleGroup` with `single` selection. The sliding thumb is mkui-specific motion (Miky design doc allows it). |

### FilterChip → shadcn `Toggle` (composition)

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `FilterChip` | `Toggle` (the toggleable button primitive) with chip styling | `mkui_wgpu::components::filter_chip` (wrapper) |
| **Geometry** | 24px tall, full-rounded | shadcn `Toggle` is `h-10`, `rounded-md` | — |
| **State** | off (panel bg + border) / on (accent-container + accent text + ✕) | shadcn `Toggle` has `data-state=on/off` | `ToggleState::{Off, On}` |
| **Notes** | Composition: `Toggle` + `Badge` (count) + dismissable affordance. Ship as `mkui_wgpu::components::filter_chip` wrapping `Toggle`. |

### Tabs (pane + sub) → shadcn `Tabs`

| Property | Miky design | shadcn equivalent | mkui implementation |
|---|---|---|---|
| **Canonical name** | `Tabs` (pane) + `Tabs` (sub) | `Tabs` ✓ | `mkui_wgpu::components::tabs` |
| **Variants** | pane (pill-shape, 26px) + sub (smaller, 22px, sliding thumb) | shadcn ships one default look + the user re-styles | `TabsVariant::{Pane, Sub}` + shared internals |
| **Notes** | shadcn `Tabs` is the right canonical. Miky's pane/sub split = sizes/density. |

---

## Structural

These don't map cleanly to single shadcn components — they're application-shell compositions. shadcn doesn't ship a "Sidebar" or "Titlebar"; those are app-specific layouts. The mkui implementation exposes them under their Miky names, but their *children* are shadcn-aligned.

### Window → no shadcn equivalent

App-shell composition. Mkui ships `mkui_wgpu::components::window` as a first-class top-level container. The amber inset border is a mkui-specific feature for client-mode lockout.

### Titlebar → composition

`mkui_wgpu::components::titlebar` wraps an HBox with: traffic lights, brand, `Input`/`SearchInput`, `Button` group, `Kbd`. All children are shadcn-aligned; the titlebar itself is mkui-named.

### Sidebar → shadcn `Sidebar`

shadcn shipped a `Sidebar` component in late 2024 (https://ui.shadcn.com/docs/components/sidebar). **This is a direct mapping.** Variants: `sidebar | floating | inset`. mkui exposes `mkui_wgpu::components::sidebar` mapping 1:1.

### SidebarFoot popover → shadcn `Popover` + `DropdownMenu`

shadcn `Popover` is the canonical primitive. The two-zone Teams+Menu layout is composition.

### NeedsYouRail → no shadcn equivalent

Application-specific right rail. Composition of `Card` rows + `Button` inline actions. mkui exposes `mkui_wgpu::components::needs_you_rail` as Miky-named primitive.

### StatusBar → no shadcn equivalent

VS Code-style bottom bar. mkui exposes `mkui_wgpu::components::status_bar` as Miky-named primitive.

### ModeSwitcher → shadcn `Tabs` (variant)

3-pill mode selector = shadcn `Tabs` with `variant="pill"`. Map to `Tabs`.

### RunControl → composition

Xcode-style. `Button` (play) + `DropdownMenu` (target) + `Badge` (worktree pill) + `Kbd`. All shadcn-aligned children; the composition is mkui-named.

### SessionButton → composition

`Button` + leading `Icon` + trailing `StatePill`. shadcn-aligned children.

### ResizeHandle → no shadcn equivalent

Drag-strip primitive. mkui-specific.

---

## Data rows

None of these are shadcn components. They're all compositions of shadcn atoms (`Badge`, `Card`, `Avatar`) plus the Miky-specific atoms (`StatePill`, `TierTag`, `LedgerChip`).

| Miky component | Composition |
|---|---|
| `PRRow` | `Row` of `[Badge(num) Text(title…ellipsis) Badge(repo) TierTag Badge(ci) Badge(codex) Badge(diff) Text(age) Dot(stale)]` |
| `IssueRow` | `Row` of `[Badge(num) Text(title) Badge*(labels) Badge(repo) Badge(state) Text(age) Dot(stale)]` |
| `AgentCard` | `Card` containing `[StatePill, Body(name+branch+transcript), Meta]` |
| `NeedsYouItem` | `Row` of `[Icon, Body(title+sub+Button(action)+age)]` with tier-1/2 attn background |
| `AuditRow` | 5-column grid `[Text(ts) Text(actor) Text(verb) Text(what)+LedgerChip Badge(link)]` |
| `SprintCard` | `Card` with `[Header(label+dates+StatePill), Goal, 4x Tile, Bar(tier-rounds), Body(retro-lessons)]` |

mkui exposes builders for each: `mkui_wgpu::components::pr_row(...)`, `mkui_wgpu::components::issue_row(...)`, etc. They're not shipped as new components — they're row-builders over the atom catalog.

---

## Composites

| Miky component | Composition | shadcn equivalent |
|---|---|---|
| `ClientBanner` | `Banner` with auto-dismiss timing | shadcn `Alert` (no auto-dismiss, manual close); mkui adds `auto_dismiss_ms` param |
| `Advisory` | `Alert` with leading icon + restart action | shadcn `Alert` direct mapping |
| `BatchHeader` | `Card` + `Progress` bar + `Button` group + `Kbd` | shadcn `Card` + `Progress` |
| `GatesStrip` | 5-tile horizontal strip | No shadcn equivalent; mkui-specific |

---

## Summary table — full Miky catalog with canonical mkui-wgpu API

| Miky name | mkui-wgpu canonical | shadcn primary | First-class component? |
|---|---|---|---|
| Chip | `badge` | `Badge` | Yes (as Badge); `chip` is alias |
| Dot | `dot` | (none) | Yes (Miky-specific) |
| StatePill | `state_pill` | (none) | Yes (Miky-specific) |
| RoleBadge | `badge` w/ `variant=Role` | `Badge` | No (variant of Badge) |
| TierTag | `badge` w/ `variant=Tier` | `Badge` | No (variant of Badge) |
| LedgerChip | `ledger_chip` | `Badge` w/ icon | Borderline (could fold into Badge) |
| Kbd | `kbd` | `<kbd>` semantic | Yes |
| Avatar | `avatar` | `Avatar` | Yes (direct mapping) |
| Button | `button` | `Button` | Yes (already exists in mkui-wgpu) |
| SearchInput | `input` w/ slot APIs | `Input` | No (composition) |
| SegmentedControl | `toggle_group` | `ToggleGroup` | Yes (direct mapping) |
| FilterChip | `filter_chip` | `Toggle` + decorations | No (composition) |
| Tabs | `tabs` | `Tabs` | Yes (direct mapping) |
| Window | `window` | (none) | Yes (Miky-named) |
| Titlebar | `titlebar` | (composition) | Yes (Miky-named) |
| Sidebar | `sidebar` | `Sidebar` | Yes (direct mapping — shadcn has Sidebar) |
| SidebarFoot popover | `popover` + `dropdown_menu` | `Popover` + `DropdownMenu` | Yes (direct mapping) |
| NeedsYouRail | `needs_you_rail` | (none) | Yes (Miky-named) |
| StatusBar | `status_bar` | (none) | Yes (Miky-named) |
| ModeSwitcher | `tabs` w/ pill variant | `Tabs` | No (variant of Tabs) |
| RunControl | (composition) | — | No (composition) |
| SessionButton | (composition) | — | No (composition) |
| ResizeHandle | `resize_handle` | (none) | Yes (Miky-specific) |
| PRRow | `pr_row` builder | (composition) | No (row builder) |
| IssueRow | `issue_row` builder | (composition) | No (row builder) |
| AgentCard | `agent_card` builder | `Card` | No (composition) |
| NeedsYouItem | `needs_you_item` builder | (composition) | No (row builder) |
| AuditRow | `audit_row` builder | (composition) | No (row builder) |
| SprintCard | `sprint_card` builder | `Card` | No (composition) |
| ClientBanner | `client_banner` | `Alert` + auto-dismiss | Borderline |
| Advisory | `alert` | `Alert` | Yes (direct mapping) |
| BatchHeader | (composition) | `Card` + `Progress` | No (composition) |
| GatesStrip | `gates_strip` | (none) | Yes (Miky-specific) |

**Counts:**
- shadcn-direct mappings (use shadcn name + API): **8** (Badge, Avatar, Button, Input, ToggleGroup, Tabs, Sidebar, Alert, Popover, DropdownMenu, Card, Progress, Toggle — most of the catalog backbone)
- Miky-specific first-class (no shadcn equivalent): **9** (Dot, StatePill, Kbd, Window, Titlebar, NeedsYouRail, StatusBar, ResizeHandle, GatesStrip)
- Compositions / row-builders / variants (not new components): **16** (everything else)

The mkui catalog is therefore **~17 first-class components**, not 30. The "30 components" framing in `docs/downstream-consumers.md` overcounts — many "Miky components" are compositions of atoms mkui already has (Badge, Button) plus row-builders that consume them.

---

## Sprint 2 implication

**Phase 3.2's "first atoms" issue should ship the shadcn-aligned atoms, not Miky-named wrappers.** Specifically:

- ✓ **Badge** (covers Chip, RoleBadge, TierTag, LedgerChip with variants)
- ✓ **Dot** (Miky-specific, ships under its name)
- ✓ **StatePill** (Miky-specific, ships under its name)

That's 3 components, all delivering value to Miky's app immediately, all aligned with shadcn where shadcn has an equivalent. The "Chip / RoleBadge / TierTag / LedgerChip" expansion is `Badge` with variants — no new components, just more rows in the variant enum.

---

## Open questions for review

1. **Should `Badge` and `Chip` be two names for the same thing, or do we just use `Badge`?** I lean *just use `Badge`* — Miky's design doc uses "Chip" but downstream code should adopt shadcn naming. The `Chip` name can live as a doc-level alias ("we call this a chip in the design language, the API is `Badge`").

2. **Is the `StatePill` worth being its own component, or is it `Badge` with an animated `Dot`?** Could go either way. I lean *first-class StatePill* because the 9-state machine encoding deserves a typed enum-driven API. If we fold into Badge, the state encoding leaks across.

3. **How do we handle Miky's specific tier color tokens for `TierTag`?** They're in Miky's theme (`tier-mechanical` through `tier-novel-surface`). The mkui-core `ThemeTokens` struct (already in `mkui-wgpu/src/theme.rs`) would need to grow tier slots — or the consumer (Miky's app) layers them on via theme extension. I lean *theme-extension*: mkui-core stays generic, Miky's app provides a `MikyThemeExtension` with the tier tokens.

4. **shadcn's `Sidebar` API (added late 2024) — do we wholesale adopt it?** It has `Sidebar / SidebarContent / SidebarFooter / SidebarGroup / SidebarMenu` etc. Quite a few sub-components. I lean *yes, adopt the API verbatim* — even though our first implementation will be smaller. The trait shape matters for downstream familiarity.

5. **For row-builders (PRRow, IssueRow, etc.) — do they ship as `mkui-wgpu::components::pr_row(scene, rect, data, theme)` or as `UiBuilder::pr_row(self, data)` calls on the existing immediate-mode `UiBuilder<T>`?** mkui-wgpu already has `UiBuilder` (`builder.rs:668`). I lean *UiBuilder method* — consistent with existing `NumberRow` / `ListRow` patterns.

---

**Last updated:** 2026-05-21. To be reviewed by Codex alongside `docs/research/mkui-text-state-of-the-art.md`.
