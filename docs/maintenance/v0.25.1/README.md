# v0.25.1 follow-up: reviewer handoff and R14 closeout

Author: GPT-6-Astra-Pro_ChatGPT  
Date: 2026-09-28 13:29 KST (UTC+09:00)  
Revision: 2
Status: Local v0.25.1 candidate; exact-candidate validation and publication pending

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

Recommendation: `HOLD v0.25.1: exact-candidate full CI and exact-SHA hosted CI
are incomplete.` This remains a patch candidate; no tag or release was
published. The post-fetch source base was
`785304c97a19379f8f83789c0aad706d020814e5`.

| Card | Status | Result and decisive evidence |
| --- | --- | --- |
| A | `IMPLEMENTED` | Evidence routes carry exact executable, configuration, working directory, and applicable local environment. `reviewer_context_resolves_current_check_record_and_log` and `reviewer_context_routes_external_local_configuration_exactly` cover declared invocation context, including non-ASCII paths and a stale same-name executable earlier on `PATH`; foreign and stale references still refuse. |
| B | `IMPLEMENTED` | Ordinary and preservation checks expose separate target, check-record, and available log routes with requirement identity and current status. `reviewer_context_exposes_preservation_check_record_and_log_routes` and `preservation_checks_keep_preview_slots_after_ordinary_checks` cover the preservation route and preview priority. |
| C | `IMPLEMENTED` | `near_limit_profile_and_perspective_keep_the_full_packet_route` keeps the complete profile and perspective while shortening optional evidence to fit the existing 16 KiB hook envelope. |
| D | `IMPLEMENTED` | Unsupported requirements fail before canonical state is written; a legacy incompatible view stays read-only and explains recovery. `unsupported_named_requirements_are_refused_before_canonical_state_write`, `legacy_incompatible_continuation_view_explains_the_limit_without_mutating_history`, and configured positive-path regressions cover those boundaries. The current CLI supports one preservation requirement per Work. Multiple requirements are refused with the affected ID and a focused design-decision request; no historical event is reinterpreted. |
| E | `IMPLEMENTED` | Carried-mutation identity validation moved to `src/run/evidence_identity.rs`; event and error semantics are unchanged. `src/run/mod.rs` is 103,614 bytes, down from 105,548 at `c2091b9`; `src/run/state.rs` is unchanged at 85,905 bytes. The checked architecture budget is tightened to 103,614 bytes. |
| F | `COVERAGE RETAINED` | Existing R13/R14 replay, currentness, preservation, and changed-subject regressions remain in the focused suite; no broader campaign or unrelated agent-catalog work was added. |
| G | `ADAPTER HANDOFF` | The updater revision handoff belongs to its separate environment adapter. No updater repository or host integration was changed in this assignment. |
| H | `REVIEW FINDINGS ADDRESSED; RE-VERIFICATION PENDING` | A bounded source review identified an evidence-route budget edge. The code now refuses silent route loss, and the adjacent shortened-preview boundary retains its notice and full packet route when those fit; the newest regression has not been executed or independently re-reviewed. |
| I | `PENDING` | The required full local CI has not passed on the current candidate, and exact-SHA hosted CI is absent. Version metadata is updated in this branch; no tag or release was created. |

### Validation and evidence limits

- Focused local results on predecessor `6d1fba5`: `cargo test --bin exitbind --test reviewer_evidence_handoff --test cross_host_continuity --test subject_progress` — 132 unit, 4 reviewer-handoff, 18 cross-host, and 26 subject-progress tests passed. These results do not cover later fixes.
- `scripts/ci-local.sh` failed on `6d1fba5` at Clippy because a helper followed the test module. That ordering was fixed in `d4b8bfd`; Clippy passed there, but six `child_capture` cases then failed because their fixture lacked the named preservation policy. The fixture was configured in `adb6824`.
- Subsequent evidence-budget changes, including the route-retention regression in the current candidate, have not been run through the focused suite or full local CI. `cargo fmt -- --check` and `git diff --check` passed on the current local diff. The package-metadata check passed on a predecessor candidate, so it is not exact-candidate CI evidence.
- Exact-SHA hosted CI is absent. The final branch head must pass the repository's required local and hosted checks before release recommendation changes.
- Native evidence class: synthetic hook/CLI behavior only. No new paid session was selected. A source-level independent review does not establish native Claude or Codex behavior; unobserved host behavior remains unverified.
- The currently supported one-preservation-requirement-per-Work route is the boundary of this patch. A supported multi-requirement binding route needs a separate product decision.

Source implementation commit: `8937132`. The current branch head still lacks
exact-candidate full CI and exact-SHA hosted CI results.
