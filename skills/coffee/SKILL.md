---
name: coffee
description: Prepare a non-trivial coding task before implementation by selecting useful context methods and returning a bounded readiness brief; skip clear, small requests.
---

<!-- soulmate-managed-skill:v1 -->

Use Coffee as a short, fail-open preparation step, not as a mandatory interview
or execution system.

- Inspect only enough project context to decide whether preparation adds value.
  A clear, bounded request should proceed without ceremony.
- Use already available and independently authorized capabilities when useful:
  a bounded repository summary for broad context; dependency or cross-module
  inspection when relationships matter; complexity review before adding
  features, dependencies, or abstractions; a short clarification pass for
  material ambiguity; an evidence-focused review lens when verification scope
  is unclear; and an implementation-planning lens when responsibilities, state,
  or invariants need decomposition. Do not require any particular third-party
  tool for these functions. Minimization does not decide verification or data
  representation, and one runnable check is a minimum rather than a cap when
  distinct claims need distinct evidence. Respect explicit-only invocation
  rules.
- Research facts with host tools. Ask the user only for decisions that change
  the intended outcome. Preserve intentionally ambitious or experimental goals
  while challenging accidental complexity.
- When Soulmate is configured, inspect the selected agent's declared `skills`
  as intent, then return a readiness brief containing the goal, useful context,
  unresolved decisions, recommended agent/skills, scope boundaries, and
  verification. Feed that brief into the existing `soulmate brief` or
  `soulmate plan` flow; do not create a second recipe or orchestration schema.
- When the brief recommends a subagent, name its context mode. Implementation
  roles may use bounded inherited task turns.
  Reviewers, advisers, auditors, and claim-scoped verifiers use
  `fork_turns="none"` plus a complete packet containing the claim, scope,
  authority boundary, allowed evidence, and output contract. A host-side
  subagent matcher or hook does not remove modes or history inherited from the
  parent conversation.
- Native hosts own tools, models, subagents, and permissions. Coffee grants no
  execution authority and must not block work when unavailable.

External cross-session orientation may be used only as advisory evidence. A
human must accept and narrow the Goal before handing it to Soulmate. Do not
auto-ingest raw session excerpts into Soulmate memory; repository docs and ADRs
remain architecture truth. The external system owns cross-session orientation,
while Soulmate owns the active task's bounded handoff and run evidence.
