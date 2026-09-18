# Holytail Replacement Checklist

This checklist describes the bounded Exitbind-owned path for preservation-risk
checked work. It does not authorize removing Holytail, publishing a release, or
claiming host-level enforcement. It describes the `v0.20.0` behavior.

Current conclusion:

```text
NOT READY TO RETIRE
```

The mechanism is covered by deterministic tests below. Packaged-resource
isolation (a temporary HOME/project that cannot inherit standalone Holytail) and
a live host smoke have not been executed, so neither packaging nor live
replacement is claimed.

## Covered

| Claim | Evidence |
| --- | --- |
| Accepted preservation requirements are first-class checked-run input and require a functional check policy. | `src/run.rs` start/successor refusal; `src/run_state.rs` replay refusal; `preservation_without_functional_policy_is_refused_at_creation_and_replay`. |
| Ordinary functional checks cannot satisfy preservation by accident. | `src/run_exit.rs` `preservation_missing` / `preservation_failed`; `failed_preservation_blocks_acceptance_without_claiming_functional_failure`. |
| Evidence applies only to the tested inputs present at acceptance. | `src/run_inputs.rs` (`product-root-files-v1`); v6 checks, approvals, protections, and acceptance record `inputsSha256`; `source_or_checker_edit_without_new_submission_invalidates_reuse_and_acceptance`. |
| A saved packet permits skipping work only after validation against current state and inputs. | `src/work_packet.rs` `work validate`; `validated_packet_reuses_unchanged_evidence_without_rerunning_checks` (independent execution counters unchanged). |
| Help distinguishes lead actions from owner decisions. | `humanHelp.ownerDecision` in residual packet v2. |
| Terminal runs are history, not continuation permits. | `InputContext::Historical` in `src/run.rs`; `terminal_history_never_authorizes_current_continuation`. |
| Changed requirements cannot inherit predecessor evidence. | `changed_preservation_policy_cannot_inherit_predecessor_evidence`. |
| Format change is explicit. | Run-event format 6; `pinned_v0_18_reader_refuses_v6_and_current_reader_keeps_v5_guarantees` (runs when `EXITBIND_V018_BIN` names a pinned 0.18.0 binary). |

## Deliberately Retired Presentation

| Presentation | Replacement |
| --- | --- |
| Standalone Holytail as a required user-facing install for this path. | Exitbind preservation requirements and checks inside the checked-run lifecycle. |
| Summary-based resume instructions. | Versioned residual packets generated from recorded run state and validated before reuse. |

## Unsupported Gap

| Gap | Status |
| --- | --- |
| Packaged-resource isolation from standalone Holytail. | Not executed; a `SKILL.md` string assertion is not this test. |
| Live host smoke with a native lead and reviewer. | Not executed. |
| Inputs outside `product-root-files-v1` (ignored files, files outside the product root, environment, remote or time-dependent conditions). | Not covered; declare such inputs inside the product root or do not rely on reuse. |
| Universal semantic equivalence between prose and implementation. | Not claimed; accepted requirements still need explicit checks and lead acceptance. |
| Owner-approved Holytail retirement, uninstall, release, or tag publication. | Not performed. |
