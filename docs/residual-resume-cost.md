# Residual Resume And Cost Evaluation

Exitbind can now project a deterministic residual packet from recorded work
state. The packet is not an LLM summary and does not depend on the original
conversation. It reports the current work identity, subject identity, facts
already established, evidence still valid for that subject, remaining
obligations, the next action, and actions the next agent should not repeat.

## Current deterministic fixtures

The current local fixtures exercise the mechanism, not broad productivity.

| Scenario | Observed reuse | Duplicate work avoided |
| --- | --- | --- |
| Fresh process resumes after implementation and passing current check | `work resume` returns reviewer as the next action and marks the current check as still valid | no duplicate check request |
| Fresh process resumes after current review approval | `work resume` returns lead acceptance as the next action and marks review as already valid | no duplicate review request |
| Subject changes after a prior passing check | `work resume` returns check as the next action and does not list the old check as reusable | stale evidence is not reused |

Measured locally:

- duplicate check executions avoided: observable in the first scenario;
- duplicate review executions avoided: observable in the second scenario;
- stale-subject reuse prevented: observable in the third scenario.

Not measured in this milestone:

- token cost;
- model calls;
- human time;
- host-specific session startup cost;
- frontier-agent correctness against and without Exitbind.

## Future matched evaluation contract

The future comparison target is **cost to accepted result**.

Use the same task, repository, frontier host, model policy, and deterministic
checks for both arms:

1. frontier agent with good project instructions and existing tests/CI;
2. same setup plus Exitbind residual resume.

Record only metrics that are actually available from the host or artifacts:

- input and output tokens;
- model calls;
- subagent calls;
- tool calls;
- repository reads;
- check executions;
- review executions;
- wall-clock time;
- human interventions;
- rework count;
- duplicate actions;
- final correctness;
- false acceptance;
- false refusal.

Missing usage data is `not measured`, not zero and not an estimate from text
length. A session restart alone must not invalidate valid evidence; a changed
subject must invalidate evidence when Exitbind's current subject rules require
it.
