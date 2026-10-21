# Prose vs Mechanism Audit

Status: post-v0.18 audit note. This document records candidates only; it does
not add a CLI command, schema field, or runtime guarantee.

Updated: 2026-10-21. Rows re-checked against the current release; the preservation
candidate is now implemented (see "Completed candidate").

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
| Bind checks, reviews, and lead decisions to the current subject; reject stale evidence. | DETERMINISTIC_PROTOCOL | Exitbind | Mechanical for checked runs (current run-event format 8): checks, reviews, protection, and acceptance bind the Accepted Subject and tested-input identity, and acceptance recomputes it (REFERENCE.md, run-event format section). Saved packets are reused only after `work validate` returns `usable`; residual packets keep stale-subject evidence out of reuse (`residual_packet_keeps_stale_subject_evidence_out_of_reuse`). |
| Do not claim projected skill bytes prove discovery, activation, compliance, or isolation. | DOCUMENTATION_ONLY | Host lead | The boundary is documented, but activation assurance is not yet a first-class state. |
| Inspect fresh-session discovery, invocation, observed behavior, and outcome when the governed path is selected. | HOST_NATIVE_CAPABILITY | Host lead | Exitbind can record observations only when the host exposes them. |
| Activate Holytail selectively for preservation-risk work, freeze accepted meaning, and read it back. | SEMANTIC_JUDGMENT | Host lead | Choosing preservation-risk work stays the Lead's judgment. Once chosen, the obligation is run state: `work begin --preserve-requirement ID:TEXT --preservation-check-command COMMAND` records it (since f561c2c), and the route policy ships as `skills/exitbind/references/preservation.md`. |
| Feed failed or missing preservation evidence into the Exitbind lifecycle as refused or blocked. | DETERMINISTIC_PROTOCOL | Exitbind | Mechanical since f561c2c: preservation-required state is machine state, and failed or missing preservation evidence blocks acceptance with gate reasons `preservation_failed` / `preservation_missing`. |
| Keep Coffee out of the new user-facing path while preserving old compatibility. | LEGACY_COMPATIBILITY | Exitbind distribution/setup | Compatibility assets remain; new initialization should not depend on Coffee. |

## Completed candidate

f561c2c ("Add preservation checks to work acceptance") implemented the
candidate this audit named: machine-readable preservation-required state for
governed runs. A Work started with `--preserve-requirement` and
`--preservation-check-command` cannot reach `EXIT READY` while that evidence is
missing, failed, or bound to a stale subject. Tests in the
`goal_progress_presentation` target cover it, including
`failed_preservation_blocks_acceptance_without_claiming_functional_failure`,
`preservation_checks_rerun_after_rework_changes_subject`, and
`changed_preservation_policy_cannot_inherit_predecessor_evidence`.

History: it was chosen because Exitbind already owns acceptance and evidence
validity, and a model could pass ordinary checks while forgetting an accepted
invariant. This audit names no further candidate.
