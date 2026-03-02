# Semantic Harmonization Charter and Invariant Baseline

Status: Active
Program: `asupersync-3cddg` (SEM-00)
Task: `asupersync-3cddg.1.1`
Scope Owner: Runtime Core maintainers + active SEM contributors
Decision Record Thread: `coord-2026-03-02`

## 1. Purpose

This charter defines the governance boundary and non-negotiable semantic
baseline for the Semantic Harmonization Program. It is the operational source
used to align runtime behavior, documentation, Lean artifacts, and TLA checks
without moving targets during execution.

## 2. Program Scope

In scope:
- Canonical semantics definitions and rule IDs for runtime-critical behavior.
- Cross-artifact alignment work (runtime/docs/Lean/TLA) against one contract.
- Evidence-driven verification and anti-drift gates.

Out of scope:
- Feature-surface expansion unrelated to semantic harmonization.
- Backward-compatibility shims for superseded semantics.
- Ad hoc semantic edits that bypass SEM governance.

## 3. Goals and Non-Goals

Goals:
- `SEM-GOAL-001`: one canonical semantic contract with stable rule IDs.
- `SEM-GOAL-002`: deterministic, replayable evidence for semantic claims.
- `SEM-GOAL-003`: explicit ownership and escalation for unresolved ambiguity.
- `SEM-GOAL-004`: machine-checkable anti-drift gates in CI.

Non-goals:
- `SEM-NONGOAL-001`: optimize for historical API compatibility.
- `SEM-NONGOAL-002`: accept implicit behavior without contract text + evidence.
- `SEM-NONGOAL-003`: defer ambiguity resolution to undocumented tribal memory.

## 4. Non-Negotiable Invariant Baseline

These invariants are normative and must be referenced by downstream SEM work.

- `SEM-INV-001 Structured Ownership`:
  every task/fiber/actor is owned by exactly one region.
- `SEM-INV-002 Region Close Implies Quiescence`:
  region close completes only when no live children remain and all finalizers
  have finished.
- `SEM-INV-003 Cancellation Protocol`:
  cancellation is request -> drain -> finalize; each phase must be idempotent.
- `SEM-INV-004 Loser Drain`:
  race/join-style combinators must cancel and fully drain non-winning branches.
- `SEM-INV-005 No Obligation Leak`:
  permits/acks/leases are never silently dropped; each obligation is committed
  or aborted.
- `SEM-INV-006 No Ambient Authority`:
  effects are capability-scoped through `Cx`; no implicit authority flow.
- `SEM-INV-007 Deterministic Replayability`:
  equivalent seeded executions must produce replayable, explainable outcomes in
  lab/runtime verification paths.

## 5. Core Semantic Definitions

- `SEM-DEF-001 Determinism`:
  for a fixed contract version, seed, and ordered external stimuli, the runtime
  produces equivalent transition outcomes and diagnostics under replay.
- `SEM-DEF-002 Structured Concurrency Ownership`:
  no orphan tasks; ownership edges are explicit and auditable through regions.
- `SEM-DEF-003 Cancellation Correctness`:
  cancel requests cannot cause silent data loss; losers and in-flight cleanup
  are drained within bounded policy.
- `SEM-DEF-004 Obligation Lifecycle`:
  obligation state transitions are explicit (`reserve -> commit|abort`) and
  externally testable.

## 6. Governance and Decision Rights

- `SEM-GOV-001` Runtime semantics authority:
  runtime-core maintainers arbitrate code-level semantic interpretation.
- `SEM-GOV-002` Formal projection authority:
  Lean/TLA owners approve projection fidelity against canonical rule IDs.
- `SEM-GOV-003` Tie-break rule:
  if runtime/docs/formal artifacts diverge, canonical contract rule text wins
  until explicitly amended via the exception workflow below.

## 7. Change Freeze and Exception Workflow

- `SEM-FRZ-001` Freeze:
  semantic-affecting edits outside the SEM dependency graph are frozen while
  SEM-01 through SEM-04 are in flight.
- `SEM-EXC-001` Emergency exception:
  allowed only for production-critical risk mitigation.
- `SEM-EXC-002` Required records for any exception:
  - impacted rule IDs
  - rationale + alternatives considered
  - rollback or forward-fix plan
  - owner and expiry
  - linked bead/thread evidence

## 8. Escalation and SLA

- `SEM-SLA-001` Critical semantic conflict:
  triage within 24 hours, decision within 48 hours.
- `SEM-SLA-002` High severity ambiguity:
  triage within 72 hours, decision within 5 days.
- `SEM-SLA-003` Medium severity discrepancy:
  triage within 7 days, decision window bounded by next SEM phase gate.
- `SEM-SLA-004` Decision publication:
  outcomes must be recorded in thread + bead history before dependent tasks
  continue.

## 9. Evidence and Communication Requirements

- `SEM-EVD-001` Every semantic claim must include reproducible evidence pointers
  (tests, reports, traces, or formal checks).
- `SEM-EVD-002` Every resolved conflict must reference rule IDs + decision
  records.
- `SEM-COMM-001` Active contributors aligned thread:
  `coord-2026-03-02`.
- `SEM-COMM-002` Current active participant set (at charter publication):
  `BlueOtter`, `ChartreuseGorge`, `CloudyHeron`, `DarkHeron`, `FrostyBarn`,
  `LilacMeadow`, `PurpleWolf`, `RoseCanyon`.

## 10. Downstream Dependency Contract

Downstream SEM tasks must:
- reference relevant `SEM-INV-*` and `SEM-DEF-*` IDs in outputs;
- declare any proposed semantic delta against this charter;
- include deterministic verification evidence before closure.

If a downstream task requires changing this charter, it must first land an
exception record under Section 7 and receive explicit governance approval.
