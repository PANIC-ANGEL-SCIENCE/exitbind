# Holytail Replacement Checklist

Holytail is Exitbind's conditional preservation capability. What this document
tracks is the retirement of the **separate installation**: the plugin, checkout,
projections, and hook registrations a project used to need alongside Exitbind.
The capability stays; the second install is what should become unnecessary.

Nothing here authorizes removing an installation, publishing a release, or
claiming host-level enforcement.

Current conclusion:

```text
EXITBIND-ONLY REPLACEMENT VERIFIED ON: Codex CLI and Claude Code, one compact
task each; OTHER PATHS UNVERIFIED
```

Packaging independence and the behavioral rows below are covered by
deterministic tests, and both claimed host paths have now been exercised once in
isolation. Production cutover has not been performed and is a separate,
owner-approved action.

## Capability matrix

Baseline: the standalone skill, its five delayed references, and its bounded
worker and semantic-reviewer roles, as inspected at their current revision.

| Required behavior | Standalone source | Exitbind equivalent | Evidence | Status |
| --- | --- | --- | --- | --- |
| Accepted meaning frozen before implementation, item by item, with its own authority | `references/semantic-contract.md` | Accepted preservation requirements are run inputs (`--preserve-requirement ID:TEXT`), each with its own check | `preservation_without_functional_policy_is_refused_at_creation_and_replay` | `INTENTIONALLY_REPLACED_WITH_EQUIVALENT_BEHAVIOR` |
| Route (`INLINE`/`FORMAL`) distinct from quality (`FULL`/`ECO`/`MODE-UNBOUND`), neither inferred from the other | Skill activation section | Resolved from the run's accepted requirements and returned as `humanHelp.preservationAssignment`; the presentation line reports it | `a_governed_run_resolves_its_own_route_and_quality` | `IMPLEMENTED_AND_TESTED` |
| Standing policy: every formal task is assigned `FULL` | Skill "standing quality selection" | Same rule, applied explicitly by the product and documented in the packaged reference | `a_governed_run_resolves_its_own_route_and_quality` | `IMPLEMENTED_AND_TESTED` |
| A child packet carries exactly one resolved assignment; a missing or conflicting one fails before work | Packet preflight | `work next` returns the resolved assignment; the packaged reference makes refusal the rule for a child that receives none | `a_governed_run_resolves_its_own_route_and_quality` | `IMPLEMENTED_BUT_UNVERIFIED` (the refusal is host-side conduct, not product enforcement) |
| Tiny, reversible, or exploratory work stays direct | Skill route section | No governed run, no percentage, and no preservation line without accepted requirements | `without_governed_work_there_is_no_progress_or_holytail_line`, `a_governed_run_resolves_its_own_route_and_quality` | `IMPLEMENTED_AND_TESTED` |
| Read-back of the exact result against the same accepted obligations | `references/delivery.md` | Each requirement's check is bound to the current subject and tested inputs and re-evaluated on every read | `source_or_checker_edit_without_new_submission_invalidates_reuse_and_acceptance` | `INTENTIONALLY_REPLACED_WITH_EQUIVALENT_BEHAVIOR` |
| Functional success cannot stand in for a broken invariant | Skill evidence limits | `preservation_failed` refuses acceptance and the report separates the passing functional check from the failure | `a_broken_invariant_is_caught_while_the_functional_check_passes`, `failed_preservation_blocks_acceptance_without_claiming_functional_failure` | `IMPLEMENTED_AND_TESTED` |
| Evidence labels distinguish observation from declaration, and neither claims independent verification | Skill evidence limits | `command_observed`, `agent_declared`, and `none` derived from the recorded acquisition | `a_governed_run_resolves_its_own_route_and_quality` | `IMPLEMENTED_AND_TESTED` |
| Worker, reviewer, and lead stay distinct; preservation is not a second acceptor | Skill roles section | Kernel roles; preservation is a check target, never an approval | `exit_transition_conformance` suite | `IMPLEMENTED_AND_TESTED` |
| Evidence stays bound to the current result and invalidates on drift; resumption reuses only what is still valid | Skill currentness rules | Run-event format 6 `inputsSha256`, residual packet v2, `work validate` | `validated_packet_reuses_unchanged_evidence_without_rerunning_checks`, `changed_preservation_policy_cannot_inherit_predecessor_evidence` | `IMPLEMENTED_AND_TESTED` |
| Terminal runs are history, not a continuation permit | Skill lifecycle rules | `InputContext::Historical` | `terminal_history_never_authorizes_current_continuation` | `IMPLEMENTED_AND_TESTED` |
| Preservation instructions and templates available without fetching a second package | Standalone skill plus five references | One packaged delayed reference, `skills/exitbind/references/preservation.md`, projected into both host skill locations and shipped in the plugin bundle | `preservation_guidance_ships_with_exitbind_and_needs_no_second_installation` | `IMPLEMENTED_AND_TESTED` |
| A standalone installation is neither required nor disturbed, and is not recreated after retirement | Standalone install/removal behavior | Exitbind installs only what it manages and leaves foreign skills and hook records alone | `an_unrelated_preservation_installation_is_neither_used_nor_disturbed` | `IMPLEMENTED_AND_TESTED` |
| `.holytail/accepted.md` and `.holytail/check.md` fallback authority | Standalone fallback workflow | Deliberately not recreated: the run state holds accepted requirements, evidence, and acceptance | `a_broken_invariant_is_caught_while_the_functional_check_passes` asserts no such file appears | `INTENTIONALLY_REPLACED_WITH_EQUIVALENT_BEHAVIOR` |
| A live host selects the skill, resolves the delayed reference, and preserves meaning end to end | Standalone live behavior | Same mechanism | One isolated task per host; see the live results below | `IMPLEMENTED_AND_TESTED` for the two exercised hosts |
| Independent semantic review by a separately isolated reviewer | `references/review.md` | Reviewer stage exists; native read-only isolation is host-dependent and unproven here | none for isolation | `IMPLEMENTED_BUT_UNVERIFIED` |
| Historical standalone evidence and its identity | Old `.holytail/` records | Kept as history; never relabelled as Exitbind evidence | `docs/legacy-compatibility.md` rule | `HISTORICAL_ONLY` |

## Live host results

One compact preservation task per host, in a disposable project under an
isolated home with no standalone preservation package reachable, using the
packaged candidate. An independent fixture oracle outside the project judged the
protected behavior; Exitbind's own READY label was not the correctness oracle.

| Observation | Codex CLI | Claude Code |
| --- | --- | --- |
| Selected Exitbind without either product being named | yes - read the bootstrap, then ran `work resume` | yes - reported the project was unconfigured and asked before any edit |
| Asked the owner for the single initialization write before mutating | yes; changed nothing until granted | yes; changed nothing until granted |
| Resolved the packaged preservation reference | yes - read `references/preservation.md` from the installed distribution | governed run begun; reference resolution not separately observed |
| Correct result passed its checks and review | yes, oracle agreed | yes, oracle agreed |
| A broken protected behavior was refused | yes - reported the tree unacceptable and stated that a verified earlier receipt does not accept the edited tree | not separately exercised |
| Frozen check could not be substituted | yes - the lead ran an equivalent suite, reported it, and still called the frozen check unresolved | not separately exercised |
| Read-only request stayed direct | yes - answered, changed no files | not separately exercised |
| Visible presentation | `[Neuro] Exitbind progress: N%.` at 0/15/40/55%; no line when no governed run applied | reported progress and the terminal state |
| `EXIT READY` written exactly | no - both hosts appended a clause on first observation | no - appended `(100%, accepted)` |

Reuse and invalidation were observed across fresh processes with execution
counters: unchanged inputs reused scope, the passed check, the preservation
result, and the review while the check command executed zero further times;
appending one line to a covered source turned the check stale, dropped the
review, and reduced reuse to scope alone.

The `EXIT READY` wording is host conduct, not product enforcement. The skill now
states the rule explicitly; a host can still ignore it, and that limit is
reported rather than claimed away.

## Separate verdicts

1. **Behavioral replacement** — every row required by the replacement scope is
   implemented and tested, except independent reviewer isolation, which depends
   on host capability and is reported rather than claimed.
2. **Distribution independence** — `VERIFIED`. The installed distribution carries
   the preservation guidance, projects it into both host skill locations, ships
   it in the plugin bundle, and a fixture with no standalone package reachable
   completes a governed preservation run.
3. **Live-host evidence** — `EXECUTED` once per host, separately, with the
   results above. Neither host's result is inferred from the other, and one
   compact task per host is a mechanism smoke, not a performance or compliance
   claim.
4. **Migration readiness** — `PREPARED`. The retirement step is the operator's
   removal of exactly their own registrations: the standalone skill directories
   projected into host skill paths, the standalone session-hook records, and the
   package entries that reinstall them. A fixture shows Exitbind neither
   disturbs those registrations while they exist nor recreates them afterwards.
   Historical evidence stays under its own identity.
5. **Real-environment cutover** — `NOT PERFORMED`. It needs its own approval,
   naming the exact registrations, a rollback ref, and a fresh session afterwards.

## Unsupported Gaps

| Gap | Status |
| --- | --- |
| A distinct, host-isolated reviewer process. | Not claimed; both runs used the workflow's reviewer stage inside one session. |
| `EXIT READY` rendered exactly by the host. | Instructed, not enforced; both hosts appended a clause. |
| Host-enforced reviewer isolation. | Not claimed; the reviewer stage exists, the isolation is the host's. |
| Child-side refusal of a packet with a missing or conflicting assignment. | Documented product rule, not product enforcement: a host can ignore it. |
| Inputs outside `product-root-files-v1` (ignored files, files outside the product root, environment, remote or time-dependent conditions). | Not covered; declare such inputs inside the product root or do not rely on reuse. |
| Universal semantic equivalence between prose and implementation. | Not claimed; accepted requirements still need explicit checks and lead acceptance. |
| Owner-approved retirement, uninstall, release, or tag publication. | Not performed. |

## Deliberately Retired Presentation

| Presentation | Replacement |
| --- | --- |
| A standalone preservation install as a required user-facing dependency for this path. | Preservation requirements and checks inside the Exitbind checked-run lifecycle, with the guidance packaged in the distribution. |
| Summary-based resume instructions. | Versioned residual packets generated from recorded run state and validated before reuse. |
| A second authority artifact pair for formal work. | Exitbind run state as the single authority. |
