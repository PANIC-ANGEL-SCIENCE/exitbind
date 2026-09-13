# What the participation pilots established

These are bounded maintainer experiments from September 2026, using released
Soulmate 0.12.0 and native coding agents. They are not an external user study or
an installation recommendation for every builder. Private task records are not
published; this summary is a report, not a publicly reproducible live-model run.
The separate [scripted proof](value-proof-methodology.md) is reproducible.

The later [real Holytail workload comparison](holytail-workload-findings.md)
uses the released 0.12.1-rc.2 binary and four independently reviewed task
copies. Both workflows completed correctly; no installation-cost recovery was
demonstrated. Its measured subprocess costs and missing human-cost evidence
are reported separately from the earlier pilots below.

## Configured agents already participated

The inspected native role definitions matched their canonical profiles. Real
named workers and reviewers completed paired configuration-validator tasks in
isolated workspaces. No role-delivery defect was reproduced. An absent project
configuration caused the optional hook to supply no context, as designed.
Profile preparation therefore must not be described as agent activation.

On two ordinary small tasks, both a competent native setup and the same setup
with Soulmate passed the same checks without re-explanation. Soulmate added
bookkeeping commands. Commands are not human actions, and active operator time,
token cost, maintenance cost, and voluntary reuse were not measured.

## Recovery exposed both a protection and a missed bug

A constructed recovery fixture supplied a failed check alongside reviewer
approval. Soulmate refused acceptance. Changed recorded artifacts and revoked
selected context also prevented the tested next handoff. Legitimate restoration
and an explicit successor recovered work while preserving prior evidence.
These failures were deliberately scripted, not observed incident frequencies.
A small native checker detected the same fixture conditions; that checker did
not implement acceptance writes or the full lifecycle protocol.

A subsequent host-driven recovery comparison used the same validator
specification and initial ten tests. Its initial false completion was scripted;
the ensuing coding, independent review, and lead decisions were real.
The native-only side discovered a real numeric bug before acceptance:

```json
{"port":80,"hosts":["ok"],"future":1e400}
```

The first implementation converted the unknown number to `Infinity` and emitted
invalid JSON with a successful exit. The Soulmate side initially passed its
checks and review and was accepted with the same defect. The native discovery
prompted an equal supplemental check for both sides: large and tiny numbers,
precision, long integers, invalid constants, and invalid numeric ports.

Both corrected implementations passed the original ten and supplemental three
tests with fresh independent review. The Soulmate correction used a new run;
the earlier accepted run and its erroneous result remained inspectable.
An accepted record did not become proof that its code had been correct.

Condition order and worker context delivery differed: one worker inherited
bounded parent context while the other received a fresh assignment. The pilot
does not establish causal superiority, statistical error rates, or lower total
operator effort for either setup.

## One native continuation remained incomplete

A separate, bounded one-arm fictional observation at reviewed source snapshot
`6911dbb` recorded an actual native worker change and a host-recorded frozen
check with exit 0. A fresh native coordinator retrieved the pending reviewer
assignment, but no native reviewer dispatch or delivery occurred and no
reviewer or lead acceptance event was appended. The coordinator reported that
its native spawn tool was unavailable; that is agent-declared capability
evidence, not independent inspection of the host tool set. A plan-only brief
cannot establish that absence.

The observation contained 10 observed Soulmate calls, 1 actual product check, 2
native trial dispatches, and 0 reviewer dispatches. A separate root read-only
verification added 2 calls, including 1 rejected absolute-ledger-path attempt;
the corrected read only confirmed that review and acceptance remained absent.
This was one fictional arm, not a matched trial or a successful continuation.
Previous equal and negative Holytail comparisons remain unchanged. No human
installation, incremental gain, savings, or omitted-review incident was
measured.

## What belongs in the product promise

Supported by these observations: existing native agents can do the work while
Soulmate handles explicit work references, reported-check bindings, and the
tested refusal/recovery transitions. The host can carry the bookkeeping rather
than asking the user to manage IDs and report paths.

Still unestablished: better code, fewer recurring human interventions, lower
total cost, preserved root conversations, or an installation that pays for
itself for both novice and experienced builders. A strong native setup remains
a valid alternative. Another green fixture alone cannot settle that question.

The current first-use guidance asks one work question and leaves the protocol
to the host. Its aim is fewer concepts before first value; this is a design
choice informed by the pilots, not a measured usability improvement.
