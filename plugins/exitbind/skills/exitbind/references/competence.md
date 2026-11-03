<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->

# Task judgment, when relevant

These are optional lenses for the current Lead, Worker or Reviewer, not new
roles or a fixed sequence. The accepted goal, assignment, host permissions,
personal profile and existing evidence and acceptance routes remain in force.
Use only the part that helps the current decision.

Conditional judgments, only when relevant:

- [assurance](../resources/judgments/assurance.md): review preparation,
  verification, architecture, security and reliability
- [context-lifecycle](../resources/judgments/context-lifecycle.md)
- [product](../resources/judgments/product.md): public entry documentation,
  user research, comparisons and interface decisions
- [quantitative](../resources/judgments/quantitative.md)
- [workflow](../resources/judgments/workflow.md): failure recovery, version
  decisions and optional managed agent configuration

These references add no role or authority.

## Frame and reuse

Identify the desired observable result, constraints and unresolved decisions.
Look for an existing implementation, accepted decision or qualified observation
before inventing a new one. Reuse a fact only while its source, environment and
relevant dependencies still match. If a consequential premise is uncertain,
choose a small permitted observation that distinguishes plausible alternatives
before making dependent changes. Clear tasks can proceed directly.

## Divide work and place responsibility

Choose direct work or a bounded delegation by coupling, uncertainty and actual
executor capability. Keep shared decisions and integration with their owner;
give a recipient sufficient inputs, scope, outcomes, checks and return limits.
Trace a changed contract from producer through transformations to consumers,
including relevant sibling and packaged paths. Put behavior and effects with
their existing owners; use types, validation or focused tests when correctness
otherwise depends on call order or a remembered convention. Split a distinct
responsibility when needed, not merely to reduce file size.

## Design evidence and recover

For each material claim, choose evidence that rejects a plausible wrong result,
including applicable invalid, bypass and preservation cases. Resolve the real
command, inputs and environment; bind results to the current artifact and state
what was observed. Separate source inspection, execution and independent review.
When a check fails, identify the failing result and use the smallest useful
discriminator before editing or repeating a broad run. Preserve still-valid
independent evidence. Repeating the same failure with unchanged inputs and no
new observation does not establish progress; report uncertainty or escalate the
actual decision when a safe recovery is not known.

## External use and integration

For a path used by an external person or agent, inspect its shortest path to a
useful result, required concepts and configuration, safe defaults, visible
status, and recovery from likely errors. Follow the connection between source,
projection, discovery, invocation and observed behavior; a copied file or
passing internal test alone does not prove a user or host used it. Distinguish
human evidence, self-use and simulations, and state remaining limits.

## Research and adoption

Separate a decision that needs direct user evidence from one answerable from
existing sources. Compare relevant alternatives, their practical costs and
uncertainty; do not describe simulated use as external adoption. For published
sources, keep attribution and licensing with any reused material.

## Security, publication and release

Trace the real trust boundary, principal, destination and allowed effect. Use
existing protections and verify only within authorized scope. Before sharing or
publishing, check the intended audience, exact material and authority; protect
private data and preserve user-owned files. For release or rollback, distinguish
the reviewed source, built artifact, installed bytes and actual running version.
The [optional publication checks](../resources/publication/README.md) require
the caller's policy and never install a Git hook.

## Reliability, resources and triage

For a failure, distinguish product behavior, test or environment defects,
resource exhaustion and missing information. Inspect the smallest failing
stage and preserve its output before a retry. Check ownership and available
resources before expensive concurrent work; do not remove another task's files
or processes. Reuse current qualified evidence and identify what changed before
rerunning an expensive check.

## Optional native operations and diagnostics

Use [same-thread continuity](../resources/workflows/continuity.md) and
[bounded dispatch](../resources/workflows/dispatch.md) when selected.
For [session-log analysis and optional repository packing](../resources/analysis/README.md)
or [checkout-bound Cargo targets](../resources/verification/README.md), use the
on-demand references; they do not collect history or qualify results automatically.
The [implementation economy reference](../resources/economy/instructions.md)
applies only when the owner selects that mode for the implementing role.
