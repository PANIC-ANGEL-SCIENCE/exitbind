# Prose vs Mechanism Audit

Status: post-v0.18 audit note. This document records candidates only; it does
not add a CLI command, schema field, or runtime guarantee.

## Scope

This audit covers the current Exitbind skill, setup guidance, and public
control-plane documentation. It asks which correctness-relevant behaviors still
depend on model obedience even though Exitbind should eventually own them as
deterministic state.

## Classification

| Current guidance | Class | Current owner | Mechanism gap |
| --- | --- | --- | --- |
| Decide whether a task is material, governed, or tiny/reversible. | SEMANTIC_JUDGMENT | Host lead | This remains a model judgment; Exitbind should not replace it with broad automatic routing. |
| Choose tools, native workers, reviewers, and role boundaries. | HOST_NATIVE_CAPABILITY | Host lead | Exitbind records configured roles, but native execution availability is host-owned. |
| Use `work begin`, `work next`, `work return`, `work check`, and `work resume` once governed work is selected. | DETERMINISTIC_PROTOCOL | Exitbind | The ledger owns state after activation; selection before activation remains prose. |
| Keep worker completion, check result, reviewer approval, and lead acceptance separate. | DETERMINISTIC_PROTOCOL | Exitbind | Mostly mechanical today through run state and acceptance gates. |
| Bind checks, reviews, and lead decisions to the current subject; reject stale evidence. | DETERMINISTIC_PROTOCOL | Exitbind | Mostly mechanical for checked v5 runs and residual packets; keep expanding from state, not summaries. |
| Do not claim projected skill bytes prove discovery, activation, compliance, or isolation. | DOCUMENTATION_ONLY | Host lead | The boundary is documented, but activation assurance is not yet a first-class state. |
| Inspect fresh-session discovery, invocation, observed behavior, and outcome when the governed path is selected. | HOST_NATIVE_CAPABILITY | Host lead | Exitbind can record observations only when the host exposes them. |
| Activate Holytail selectively for preservation-risk work, freeze accepted meaning, and read it back. | SEMANTIC_JUDGMENT | Host lead | Preservation obligations are still mostly prose rather than first-class run state. |
| Feed failed or missing preservation evidence into the Exitbind lifecycle as refused or blocked. | DETERMINISTIC_PROTOCOL | Candidate Exitbind owner | The lifecycle can represent refusal/blocking, but preservation-required state is not yet machine-readable. |
| Keep Coffee out of the new user-facing path while preserving old compatibility. | LEGACY_COMPATIBILITY | Exitbind distribution/setup | Compatibility assets remain; new initialization should not depend on Coffee. |

## Highest-Value Candidate

The next mechanical migration should be machine-readable preservation-required
state for governed runs.

Why this candidate:

- Exitbind owns the acceptance lifecycle and evidence validity.
- Preservation state can be represented without becoming a host runtime.
- A concrete counterexample exists: a model can pass ordinary checks while
  forgetting an accepted compatibility or behavior invariant.
- It can be regression-tested by requiring a preservation obligation and
  proving the run cannot reach `EXIT READY` while preservation evidence is
  missing or bound to a stale subject.
- It improves acceptance integrity without adding more prose to the skill.

Do not start multiple migrations from this audit. The next increment should
choose one minimal state representation, one counterexample fixture, and one
acceptance gate.
