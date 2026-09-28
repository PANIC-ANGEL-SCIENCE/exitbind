# v0.25.1 follow-up: reviewer handoff and R14 closeout

Author: GPT-6-Astra-Pro_ChatGPT  
Date: 2026-09-28 10:06 UTC
Revision: 6
Status: v0.25.1 patch candidate; recommendation remains on hold while required host validation is unverified

## Start here

This is the operator-selected Codex follow-up, not an installed agent skill or
an instruction to resume development whenever someone clones the repository.
Read the repository [AGENTS.md](../../../AGENTS.md) and
[CONTRIBUTING.md](../../../CONTRIBUTING.md), then the relevant work card below.
This packet consolidates the earlier narrow reviewer-handoff brief and adds
bounded R14 closeout work. It does not reopen the complete W1-W7 roadmap.

Inspected product baseline:
- branch: `fix/r13-v0.25.1-hardening`;
- reviewer-handoff commit: `c2091b93445409dc2680f4d64e22b6d8269f378c`;
- R14 parent: `046f55d19d9145b66784d15d2747271e29f20053`;
- stable/main at inspection: `ab87e2ae18f43ecaa8102ea7fa2fc2cae958340f`.

Fetch current refs first. Preserve newer commits and uncommitted work; do not
reset to these advisory references. Keep the delivered handoff module and the
R13/R14 check, replay, currentness, child-origin and authority repairs.

## Work cards and order

| Order | Responsibility | Work card |
| --- | --- | --- |
| 1 | Usable reviewer evidence routes, including preservation checks | [01_REVIEWER_HANDOFF.md](01_REVIEWER_HANDOFF.md) |
| 2 | Early whole-goal compatibility diagnosis and focused responsibility extraction | [02_R14_CLOSEOUT.md](02_R14_CLOSEOUT.md) |
| 3 | Exact-candidate validation, honest native evidence, and the separate updater handoff | [03_VALIDATION_AND_ADAPTER.md](03_VALIDATION_AND_ADAPTER.md) |

Use one coherent owner for each changed responsibility. A second agent is not
needed merely because this packet has multiple files. An independent reviewer
remains read-only and separate from the implementation owner.

## Authority and stop boundary

Implementation, focused tests, branch commits and ordinary CI are selected.
This assignment does not authorize main merge, tags, release publication,
branch-protection changes, default-server cutover, new paid model sessions,
billing changes, dotagents removal or a personal-environment rewrite.

The external updater issue has its own ownership boundary in card 03. It is
not permission to put a deployment framework into Exitbind or to make a private
environment adapter a prerequisite for the public product.

Close reproduced in-scope failures, including adjacent cases of the same
cause, rather than only reporting them. If a repair needs a new authority model,
storage protocol, broad migration or a different product decision, stop that
item with a narrow reproducer and follow-up boundary. Do not waive evidence or
loop through reviewers to manufacture a pass.

Finish with `RECOMMEND v0.25.1` or `HOLD v0.25.1: <exact product blocker>`, plus
separate native-observation and external-adapter statuses. Neither outcome
publishes a release. An unavailable paid-host observation is not permission to
spend and must not be relabelled as a passing native test.

## Public/private boundary

This packet contains product-level instructions only. Keep private repository
names, personal agent catalogs, server locations, account balances, real work
handles, transcripts and raw logs out of public reports and commit messages.
Use synthetic fixtures and existing approved private reporting channels for
sensitive operational evidence. Do not inject these maintenance cards into
normal SessionStart context or distributed end-user skills.

## Current status report

Recommendation: `HOLD v0.25.1: normal non-publishing WSL validation remains
unverified.` This remains a patch candidate; no tag or release was published.
The post-fetch source base was `785304c97a19379f8f83789c0aad706d020814e5`.
The branch head before this status update was
`7ea99431f0cc8d5a129d73a025ed098d5960fea3`. Resolve the commit containing this
report and inspect its own exact-SHA checks; this text does not substitute for
a result attached to that commit.

| Card | Status | Result and decisive evidence |
| --- | --- | --- |
| A | `IMPLEMENTED` | Evidence routes carry exact executable, configuration, working directory, and applicable local environment. `reviewer_context_resolves_current_check_record_and_log` and `reviewer_context_routes_external_local_configuration_exactly` cover declared invocation context, including non-ASCII paths and a stale same-name executable earlier on `PATH`; foreign and stale references still refuse. |
| B | `IMPLEMENTED` | Ordinary and preservation checks expose separate target, check-record, and available log routes with requirement identity and current status. `reviewer_context_exposes_preservation_check_record_and_log_routes` and `preservation_checks_keep_preview_slots_after_ordinary_checks` cover the preservation route and preview priority. |
| C | `IMPLEMENTED` | `near_limit_profile_and_perspective_keep_the_full_packet_route` keeps the complete profile and perspective while shortening optional evidence to fit the existing 16 KiB hook envelope. |
| D | `IMPLEMENTED` | Unsupported requirements fail before canonical state is written; a legacy incompatible view stays read-only and explains recovery. `unsupported_named_requirements_are_refused_before_canonical_state_write`, `legacy_incompatible_continuation_view_explains_the_limit_without_mutating_history`, and configured positive-path regressions cover those boundaries. The current CLI supports one preservation requirement per Work. Multiple requirements are refused with the affected ID and a focused design-decision request; no historical event is reinterpreted. |
| E | `IMPLEMENTED` | Carried-mutation identity validation moved to `src/run/evidence_identity.rs`; event and error semantics are unchanged. `src/run/mod.rs` is 103,614 bytes, down from 105,548 at `c2091b9`; `src/run/state.rs` is unchanged at 85,905 bytes. The checked architecture budget is tightened to 103,614 bytes. |
| F | `COVERAGE RETAINED` | Existing R13/R14 replay, currentness, preservation, and changed-subject regressions remain in the focused suite; no broader campaign or unrelated agent-catalog work was added. |
| G | `ADAPTER HANDOFF` | The updater revision handoff belongs to its separate environment adapter. No updater repository or host integration was changed in this assignment. |
| H | `INDEPENDENTLY RE-REVIEWED` | The budget-edge finding is closed in source. The re-review confirmed that `shortened_notice_and_route_survive_when_header_does_not_fit` fixes the prior boundary and that an unavailable route is surfaced explicitly; it found no remaining issue in that scope. |
| I | `UNVERIFIED` | Read full local and hosted CI results on the exact commit containing this report. Normal non-publishing WSL validation remains unverified, so the release recommendation stays on hold. Version metadata is updated; no tag or release was created. |

### Validation and evidence limits

- `native_continuity` passed 7/7 after its local-mode fixture began creating
  its own Git root. `skill_diagnostics` passed 9/9 after the same fixture
  boundary was made explicit. The legacy suite exposed a missing mode in the
  benchmark's own synthetic `init`; `value_benchmark` passed 6/6 after that
  command selected `--mode portable`.
- The previous full run on `f7b2003` stopped at `value_benchmark` because the
  benchmark omitted the required mode. That cause was repaired in
  `3c5e1ed`. Earlier fixture failures were repaired in `a1448e1` and
  `f7b2003`; those failed SHAs remain historical failures.
- The next full run on `17f6a69` stopped before a `host_bridge` child started:
  Linux returned `ETXTBSY` while the test launched its freshly copied binary.
  The fixture now stages executables atomically and uses the existing
  ETXTBSY-only bounded runner. The focused `host_bridge` suite passed 11/11.
- Full local CI on `d6e6a45` passed the main tests, legacy compatibility,
  value proof, and drift warning, then failed at the real-tmux native-handoff
  check because tmux could not start the controlled child. The exact full run
  on `7ea99431f0cc8d5a129d73a025ed098d5960fea3` reached the same boundary; a
  task-isolated tmux probe could not open its Unix socket in this execution
  environment. Treat native handoff as unverified locally; check the exact
  hosted result.
- Running the release-reference stage separately exposed false positives for
  preserved historical compatibility facts and the branch identifier. The
  checker now exempts only those exact documented cases and excludes its own
  policy source; arbitrary stale references remain checked.
  Consult the exact result for the commit containing this report.
- The re-review covered the evidence-route budget repair at `a1448e1`. The
  later changes through `3c5e1ed` add isolated test Git roots and make the
  benchmark's internal initialization mode explicit. Review did not claim
  native host behavior.
- For the commit containing this report, consult the exact result of
  [`scripts/ci-local.sh`](../../../scripts/ci-local.sh) and the hosted
  [CI workflow](../../../.github/workflows/ci.yml). This report alone is not a
  passing check. WSL was not observed; synthetic hook/CLI evidence does not
  establish native Claude or Codex behavior. No new paid session was selected.
- The currently supported one-preservation-requirement-per-Work route is the
  boundary of this patch. A supported multi-requirement binding route needs a
  separate product decision.
