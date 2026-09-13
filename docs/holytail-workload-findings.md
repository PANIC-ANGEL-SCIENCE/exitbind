# A real small change, with and without Soulmate

On September 7, 2026, a bounded maintainer comparison used a real Holytail
maintenance task: make its existing validator discover relative links in
companion documentation, including nested documents. The previous operating
guide asked for a separate manual check. The intended improvement removes that
extra step and adds a regression that detects the original omission.

The task used Holytail source `14d0c9593148e6a9b7a38ef8053beaa3057686cc`
and the released Soulmate 0.12.1-rc.2 binary. Four independent source copies
received the same acceptance criteria, worker model, role boundaries, and
project check. Workers used gpt-5.6-luna with maximum reasoning effort. The
baseline already had native agents, Holytail, durable handoffs, actual checks,
and independent review. It was not deprived of context to favor Soulmate.

## What happened

| Condition | Existing native workflow | Same workflow plus Soulmate |
| --- | --- | --- |
| Ordinary small change | Correct change, check and review passed; lead accepted. | Correct change, check and review passed; lead accepted. |
| Fresh-context resumption after one planned input change | Detected the new broken link, repaired it, preserved prior implementation and evidence; lead accepted. | Same product outcome and reconstructed task/check/next action; lead accepted with linked records. |

The resumption input was deliberately introduced after the first worker result
and review: a new document linked to a missing README path. Both real checks
failed, and both fresh workers repaired it. This is an injected change, not an
observed incident rate. No failed-check acceptance was attempted in these
four observations, so they do not add a new observed refusal to the separate
[scripted proof](value-proof-methodology.md).

Independent product review used a frozen 41-case oracle and scoped mutation
checks. All four implementations passed. Revised snapshots preserved the
original implementation bytes, allowing prior evidence to be reused; the
new link and exact check were verified again. Review approval and the lead's
subsequent acceptance were recorded separately.

## Cost that was actually observed

The single release installation subprocess took about 1.03 seconds in this
environment. This excludes learning, choosing configuration, model work,
review, and human attention; it is not a new-user installation estimate.

| Recorded host project-check invocations | Native workflow | With Soulmate |
| --- | --- | --- |
| Ordinary condition | 1 | 1, plus 13 Soulmate commands |
| Resumption condition | 3 | 3, plus 27 Soulmate commands and one untimed rejected host spawn |

The added Soulmate subprocesses took about 0.45 seconds and 1.08 seconds,
respectively. Those sums exclude the installer and untimed configuration,
host-glue, reviewer, and reasoning work. Command counts are not human actions.
Worker wall times include scheduling and waiting, so they are not reported as
causal savings. Token charges, active operator time, and ongoing maintenance
cost were unavailable.

The host retained a previously used native task name. Resolving the exact
mapping required explicit successor records; a later fresh worker also needed
a new mapping. This exposed integration friction. The record retained the
changes, but did not eliminate the host's naming constraint.

## What this supports

**No installation-cost recovery was demonstrated in this sample.** Both
workflows produced correct changes and reconstructed the required task state.
Soulmate added structured check/decision bindings and recording operations;
no quality or reconstruction advantage was observed. Total human payback is
inconclusive because material costs were not measured.

The screening budget was fixed at four observations, with one planned
resumption change; no extra trials were added to obtain a favorable result.
Active-versus-waiting time was not captured well enough to verify the declared
execution/setup time caps. Setup metadata also contained a truncated task hash
and named a sibling installer checkout; direct byte comparisons established
the full task identity and exact pinned installer bytes. These provenance
defects remain in the private records rather than being rewritten away.

This is a small maintainer report, not a blinded population study or a
publicly reproducible live-model benchmark. Raw task records remain private.
The Holytail code improvement is useful on its own; it does not establish that
Soulmate caused the improvement. A strong existing native workflow remains a
valid choice. A future payback claim needs ordinary-use evidence that a
recurring loss disappeared, including setup, repair, and review costs.

## Token-economics retrospective — September 7, 2026

The tested hypothesis was that Soulmate might reduce total model-token usage enough to contribute to installation payback. The native baseline was competent: it already used agents, Holytail, durable handoffs, real checks, and independent review. The original comparison fixed a four-observation screening budget with one planned fresh-context resumption change, and gave both conditions the same acceptance criteria, worker model, role boundaries, and project checks. Independent review used the frozen oracle and scoped mutation checks. Later recommendations to capture active-versus-waiting time and complete workflow costs were follow-up measurement needs, not controls applied retroactively.

The operator chose the question and budget and made the stop and publication decisions. Agents performed setup, source changes, checks, reviews, record recovery, and arithmetic. Historical token usage was recovered retrospectively from existing private host records; raw logs remain private, and no experiment was rerun.

The directly attributable combined slice was 4,043,947 tokens for A and 4,149,963 for B: B was 106,016 tokens higher (+2.6216%). The narrow P2 resume worker and reviewer slice was 1,064,197 for A and 1,057,027 for B: B was 7,170 lower (-0.6737%). Shared setup used 2,463,969 tokens, and the 11,436,168-token mixed coordinator window covered multiple arms; neither shared amount can be fairly allocated into those direct totals.

Token savings were not established; total-workflow economics remain inconclusive. This unsuccessful validation does not prove Soulmate is useless.
