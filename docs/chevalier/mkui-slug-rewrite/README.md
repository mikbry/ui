# Chevalier mission — mkui Slug rendering completion

**Status:** CHARTER v1.2 awaiting operator ratification (this PR)
**Sprint:** miky Sprint 18 Lane 2 (second chevalier mission, empirical wedge-metric data point)
**Parent tracker:** [mikbry/ui#157](https://github.com/mikbry/ui/issues/157) — Slug completion (8-step plan)
**Substrate:** [mikbry/miky#587](https://github.com/mikbry/miky/issues/587) — chevalier epic + Finding L (dame du lac pattern) + Finding N (operator-authors-nothing)

## What lives here

| File | Purpose | Immutability |
|---|---|---|
| `CHARTER.md` | Mission specification: scope, oracles, YOLO scope, blocks, hard limits, phases | Immutable during mission — operator amendment only |
| `dame-rubric.md` | Per-phase BLESS criteria for dame's structured verification | Immutable during mission — operator amendment only |
| `codex-8-step-plan.md` | Frozen copy of the normative implementation plan (from #157 body at ratification time) | Immutable during mission — the `execution_oracle` |
| `reference-harness/` | Ratified independent oracle adapter (WGSL port of pinned upstream Slug HLSL) — merged 2026-07-28 as #158 (`9f76af3`) | Immutable during mission — adapter SHA pinned in CHARTER |

## Why these live in `mikbry/ui` (not strategy repo)

Per Codex review 2026-07-27 (`#587` Finding L follow-up): the dame verifier runs `git log` on the mission's ratified documents from the chevalier's own PR worktrees. Mission docs must live in the same repository the chevalier operates on. Also enforces immutability: a single ratification merge SHA pins CHARTER + rubric + Codex plan + adapter together.

## Ratification chain

1. Adapter authored + reviewed → PR #158 merged `9f76af3` (2026-07-28) — verification oracle ratified
2. **This PR** — CHARTER v1.2 + dame-rubric v1.2 + Codex plan copy — operator ratifies → mission touchpoint 1
3. Chevalier dispatched — chevalier + dame loop autonomously per CHARTER
4. Operator visual smoke at COMPLETION — mission touchpoint 2

## Reading order

1. `codex-8-step-plan.md` — what the chevalier is building (execution scope)
2. `CHARTER.md` — how the chevalier operates (mission contract)
3. `dame-rubric.md` — how the deliverable is verified (dame's decision procedure)
4. `reference-harness/README.md` — what the verification oracle actually is
