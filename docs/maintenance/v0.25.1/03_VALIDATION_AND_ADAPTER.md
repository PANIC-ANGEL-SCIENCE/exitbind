# Validation, reporting and the separate delivery-adapter follow-up

Author: GPT-6-Astra-Pro_ChatGPT  
Date: 2026-09-28 13:29 KST (UTC+09:00)  
Revision: 1  
Status: Execution boundary; not evidence of completed implementation

## G. Keep the updater-version issue in its actual owner

Reported R14 adapter case: a long-running updater loaded old Python code,
fast-forwarded its own checkout, then applied the new catalog using the old
in-memory installation procedure. Validation refused; a new invocation of the
current updater succeeded. A second successful invocation recovered that host;
it did not establish that future updater upgrades are safe in one invocation.

This is not an Exitbind ledger defect. Do not add repository fetching, package
management or personal host synchronization to Exitbind to address it.

If the implementation session already has explicit authority for the separate
environment repository, close this cause there as an isolated companion change.
Otherwise produce the following bounded handoff through the approved private
channel. This public instruction does not grant new external-repository or
server-write authority.

Required invariant: the revision of the applying code matches the revision of
the catalog being applied, or the updater stops before installing new content.
A stable bootstrap plus an exact-revision applier, or one guarded restart into
that revision, can be sufficient. Reuse existing sync/locking mechanisms.

Cover: old updater fetches an update that changes the applier; locked revision
changes before apply; applier validation failure; preserved rollback/retirement
ownership proof; applied marker advances only after exact validation; already
current state is a no-op. Keep the lock across the version handoff, bound any
restart, and do not lose the prior catalog proof needed for safe retirement.
Do not rely on another timer tick as the correctness mechanism.

Report this separately as `ADAPTER_FIXED`, `ADAPTER_HANDOFF`, or
`ADAPTER_UNVERIFIED`. An external adapter handoff must not be reported as a
closed cause, but it is not by itself an Exitbind patch-release blocker.

## H. Validate the product without a new paid-host campaign

Start with the reproductions in cards 01 and 02 and focused regressions. Use one
read-only independent review of the coherent final delta, then at most one
bounded repair/re-review for a reproduced in-scope finding. No reviewer-of-
reviewer chain, speculative redesign or new full-repository audit.

Group failures by causal responsibility. A recurring same-cause failure is not
closed merely because it has another report. A materially new authority or
workflow family needs an explicit follow-up boundary, not indefinite expansion.

Use one already-available native Codex reviewer if it answers the remaining
handoff question within existing access. Give it only the normal generated
assignment, intended profile, emitted routes and permitted tools. It must read
records without a Lead-built second dossier or implementation-source archaeology
as a normal-use workaround. An adverse verdict about the actual artifact is not
a handoff failure. A missing supported host facility remains a stated limitation;
do not change permissions, subscriptions or billing to force the observation.

Retrieve any authorized retained observation of the original Claude candidate
before claiming a new one is needed. Keep separate:
- synthetic hook/CLI regression;
- fresh Codex behavior;
- actually recorded Claude behavior;
- unavailable or not-run native observation.

Do not infer Claude success from Codex, CI's unrelated native-handoff stage or a
commit message. No new paid session or token experiment is selected.

## I. Final candidate, evidence and finite closeout

After code, tests and required documentation settle, run applicable repository
checks through the existing shared scripts, architecture budgets and exact-SHA
hosted CI. Reuse existing results for unchanged premises; do not rerun full CI
on every local edit. The full patch recommendation needs the normal non-publishing
WSL validation on the final candidate, not the earlier R14 SHA's result.

These instruction files are tracked repository changes. They do not inherit a
prior tree's check, review or acceptance just because they are documentation.
Do not weaken tested-input coverage or move live acceptance events to fit them.
Source, executable, installed guidance, host-presented context and actual behavior
remain separately identified. A source review is not a server cutover.

Deliver one compact completion matrix with A-F dispositions, exact base/final
SHAs, reproductions, regression names/results, native evidence class, final CI
coverage and remaining blocker. Give the adapter its separate G status. Use
`FIXED`, `ALREADY_GUARDED`, `NOT_REPRODUCED`, `DEFERRED_OUTSIDE_PATCH` or
`UNVERIFIED` with evidence, never an unsupported all-green summary.

Public results contain sanitized synthetic examples and public source/CI links.
Private operational reports remain in the already-approved private channel; do
not copy raw traces, account details or private repository references here.
Mark this assignment completed or superseded after delivery so later sessions do
not replay it. Preserve original R14 observations at their original subjects.

End with `RECOMMEND v0.25.1` or `HOLD v0.25.1: <exact product blocker>`. Keep
unobserved host coverage visible instead of inventing a pass or demanding
unbudgeted paid runs. There is no automatic merge, publication, tag rewrite,
branch-protection change or default-server replacement in this assignment.
