---
name: soulmate
description: Use Soulmate selectively for bounded handoffs, independent review, resumability, or deterministic evidence; ordinary reversible work stays direct.
---

<!-- soulmate-managed-skill:v1 -->

# Soulmate

## WHEN

Use Soulmate when a task benefits from a bounded native-host handoff,
independent review, resumability, or deterministic check evidence. Keep small,
reversible single-agent work in the existing conversation. Soulmate does not
launch models, subagents, schedulers, daemons, or arbitrary commands.

## SELECT AND ACTIVATE

Keep these states distinct: available means Soulmate exists; discovered means
the current host/session can see its guidance; selected means the lead chose it
for this task; activated means a successful high-level `soulmate work begin`
returned a work handle and next action. At the start of each material task,
classify once whether Soulmate is available and discoverable and whether a
governed trigger materially matters. For an eligible trigger, select Soulmate
automatically and run `soulmate work begin` before any scoped implementation or
mutation; proceed only after it succeeds and returns a work handle and next
action. The user need not name Soulmate again. If unavailable or activation
fails, stop before scoped work and report the exact failing layer or capability
as an explicit blocker; never silently downgrade or relabel a material trigger
as advisory. Direct fallback is allowed only for a genuinely advisory trigger
and must state the no-activation limitation. Keep tiny, obvious, reversible
work direct without activation.

Installation, projected bytes, and setup words prove only availability or
projection. They do not prove session discovery, fresh-session discovery,
active-session reload, future compliance, selection, or activation.

## HUMAN CONTRACT

Keep the human in the existing conversation. Ask only for real installation,
permission, goal, scope, or authority decisions. Do not ask the human to carry
Soulmate commands, ledger paths, event identifiers, artifact paths, check
results, protocol versions, or recovery choices.

## AUTHORITY

The human owns intended outcome, meaningful scope, permissions, and acceptance.
The existing host owns model execution, native subagents, and permissions.
Soulmate owns bounded records, exact evidence binding, and lifecycle truth. A
worker completion is not a check result; reviewer approval is not lead
acceptance. Skill and hook presentation never proves invocation or compliance.

## HOW

After setup and configuration validation, use the machine-facing façade:

```text
soulmate work begin WORKFLOW --goal GOAL --check-command CHECK [--config CONFIG]
soulmate work next WORK
soulmate work return WORK ASSIGNMENT --outcome OUTCOME < result.txt
soulmate work check WORK
soulmate work resume
```

`begin` creates a checked run and returns an opaque work handle plus one next
action. Follow exactly one returned action. Spawn only through the host's
native worker/reviewer mechanism with the returned packet. Return role result
bytes on stdin; Soulmate allocates and validates the artifact, appends through
the strict core, and returns one next action. `check` executes the frozen
local policy and binds it to the current worker internally. `resume` is
explicit when there are zero, one, or multiple active façade works; never
guess among many.

The façade never grants permissions, starts an agent, or replaces the strict
run core. Existing `run` commands remain the advanced/manual interoperability
surface. Load the [manual reference](references/manual.md) only when debugging,
recovering, inspecting evidence, or using that low-level surface.

## STOP

Stop and ask the owning human/lead when a real goal, scope, permission,
authority, or ambiguity decision is required; when native host execution is
unavailable; when a returned action is stale or missing; or when configuration,
boundary, profile, harness, memory, ledger, target, or artifact evidence drifts.
Never fabricate completion, acceptance, check evidence, or a recovery choice.

## REALITY, DECISIONS, READINESS

Separate installed bytes, projected skill bytes, active host instructions,
execution, and outcome. A hash proves bytes only; an exit status proves that
process only; a self-report is agent-declared evidence. State unverified links
and the smallest decisive next probe. Preserve criticism while routing each
material finding as change, keep, defer, or stop under its existing authority.
Before a costly dependent phase, bind the exact target to host, configuration,
and session and stop if decisive evidence is missing or stale. Do not create a
second truth store, ingest transcripts, or promote shared memory.
