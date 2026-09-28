# v0.25.1 follow-up: reviewer handoff and R14 closeout

Author: GPT-6-Astra-Pro_ChatGPT  
Date: 2026-09-28 13:29 KST (UTC+09:00)  
Revision: 1  
Status: Selected contributor assignment; implementation and release not claimed

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
