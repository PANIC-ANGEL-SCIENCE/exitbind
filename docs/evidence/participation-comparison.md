# Maintainer comparison: method, results, and limits

This public note preserves the decision-relevant results of a bounded
September 2026 maintainer comparison of an existing native workflow with the
same workflow plus Soulmate 0.12.1-rc.2. It used four independent copies of one
real Holytail maintenance task, equal acceptance criteria, worker model, role
boundaries, project checks, and independent review. The native baseline already
used agents, durable handoffs, actual checks, and review. This was not an
external-user study or a blinded population benchmark.

Both workflows produced correct changes, passed the frozen 41-case review
oracle and scoped mutation checks, and were accepted. In one planned resumption
condition, a broken link was injected after the first review; both workflows
failed the project check and repaired it. That is a constructed change, not an
incident rate. No failed-check acceptance was attempted in these observations.
No quality or reconstruction advantage was observed, and no installation-cost
recovery was demonstrated.

The directly attributable token totals were 4,043,947 for the native workflow
and 4,149,963 with Soulmate, a difference of +106,016 (+2.6216%). The narrow
P2 resumption worker/reviewer slice was 1,064,197 and 1,057,027 respectively,
a difference of -7,170 (-0.6737%). Shared setup used 2,463,969 tokens; a mixed
coordinator window used 11,436,168 tokens. Neither shared amount can be fairly
allocated to the direct totals. The numbers do not establish token savings;
whole-workflow economics remain inconclusive.

The single installation subprocess took about 1.03 seconds in this environment.
Added Soulmate subprocesses took about 0.45 seconds for the ordinary condition
and 1.08 seconds for resumption. These timings exclude learning, configuration,
model reasoning, review, and human attention; they are not new-user setup or
payback estimates. Commands are not human actions, and worker wall time includes
scheduling and waiting. Token charges, active operator time, and maintenance
cost were unavailable. Active-versus-waiting time was not captured well enough
to verify declared execution/setup caps.

A host retained a previously used native task name, requiring explicit successor
records and a later mapping for a fresh worker. This exposed integration
friction, while the records still retained the changes. Setup metadata had a
truncated task hash and named a sibling installer checkout; direct byte
comparisons established task identity and pinned installer bytes. These
provenance limitations are retained in the interpretation, not treated as
positive results. Raw task records remain private, and no experiment was rerun
for this summary.

The Holytail code improvement is useful on its own and is not attributed to
Soulmate. A strong native workflow remains a valid choice. A future payback
claim requires ordinary-use evidence that a recurring loss disappeared and
measurement of setup, repair, review, and ongoing maintenance costs. This
unsuccessful validation does not prove Soulmate is useless.

Separate preservation replacement observations covered one compact task each
in isolated Codex CLI and Claude Code homes, with no standalone preservation
package reachable and an independent fixture oracle. Both results passed that
oracle, but each host appended a clause to `EXIT READY` on its first observation
instead of following the exact wording. Only the Codex observation separately
tested refusal of a broken protected behavior and an unresolved frozen check;
Claude reference resolution was not separately observed. Independent reviewer
isolation remains unproven, and production cutover was not performed. These two
host observations do not establish general host enforcement or replaceability.
