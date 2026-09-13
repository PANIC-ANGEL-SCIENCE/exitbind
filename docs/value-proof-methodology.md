# Reproduce a checked result

It verifies what you asked an agent to do and what came back, in the same
record.

In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.

## What the proof establishes

The [claim registry](../proof/claims.json) connects that sentence to the
[false-completion scenario](../proof/scenarios/false-completion-v1/) and its
machine-readable expected result. The scenario starts with one seeded failed
check, exercises the same CLI used in a project, attempts final acceptance,
and then repairs the run through a fresh attempt. Earlier artifact bytes remain
available. A successful rerun and reviewer approval still require a separate
lead acceptance.

The baseline also retains the check's nonzero exit status. A competent host or
person can stop there without Exitbind. The comparison tests whether that result
is part of the local acceptance record and checked when acceptance is requested;
it does not assume that Git or a host conceals failed tests.

Run the installed binary's disposable, token-free scenario:

```sh
exitbind benchmark
```

For a complete checked-work journey on a tiny product input, use the separate
[source example](first-checked-run.md#try-the-complete-example):

```sh
cargo build --locked
EXITBIND_BIN=target/debug/exitbind ./scripts/demo-checked-work.sh
```

This shell fixture observes actual failed and passing check exits and scripts
the worker, reviewer, and lead documents. It retrieves the rework assignment in
a fresh CLI process and checks that the earlier worker document remains intact.
It uses the stable `--event-id`/`--text` conveniences, removes its temporary
project, and produces no proof export. The installed benchmark and registered
scenario retain their existing claims and formats. Neither example is an
external-user experiment or demonstrates conversation preservation.

From source, the reproducible validation path for the registered proof is:

```sh
cargo test --locked
EXITBIND_BIN=target/debug/exitbind ./scripts/run-value-proof-suite.sh
```

For a change to proof schemas, compare with the existing base commit as well:

```sh
VALUE_PROOF_BASE=BASE_COMMIT cargo test --locked --test value_contract
```

Use the actual pull-request base or previous branch commit. For a newly created
branch or tag, CI compares the candidate's first parent because no previous ref
exists. The selected comparison commit must resolve; invalid bases fail. The
gate preserves every
previously pinned schema byte; a changed format needs a new versioned schema
file. Schema identifiers name format versions independently of package releases.

To retain the generated local proof instead of only displaying it, use a new
output directory:

```sh
exitbind benchmark --output proof-local
```

The fixture is synthetic. It invokes no model, uses no account, and does not
run commands in your project. The default temporary fixture is removed. An
explicit export retains the generated evidence for inspection without the
original host transcript.

## Read the evidence in order

| State | Evidence | What it establishes |
| --- | --- | --- |
| Declared | Worker `completed` submission | The named actor recorded a completion claim. |
| Reported check | Exact command and current submission binding, reported exit code | The supplied result belongs to that recorded target under the frozen check policy. |
| Reviewed | Reviewer outcome in the current attempt | A reviewer recorded its decision in the workflow. |
| Accepted | Configured lead's final `accepted` submission | The run's canonical authority recorded acceptance after the enabled guards passed. |

The historical v4 reader can locally observe the frozen command or consume a caller
report; v3 `run record-check` is caller-reported-only and never runs the command.
Neither the producer field nor a SHA-256 authenticates a caller or proves that a
reported command actually executed. The synthetic harness
separately observes its own child-process exits, so its deterministic assertions
have a stronger, explicitly bounded basis than an arbitrary imported report.

Current artifact integrity is a separate question from historical acceptance.
A historical accepted event stays recorded when a file later changes; the
current-state view must show that change. Missing observations say nothing about
whether an unobserved action happened.

## Reports and privacy

Reports stay local and separate synthetic runs from local caller reports.
They count factual recorded qualifications/refusals and retain exact evidence
identifiers. They do not infer production incidents, avoided loss, model
compliance, or false-positive rates from those counts.

Aggregate reports omit commands, goals, prompts, profiles, transcripts, paths,
and artifact contents. The [example report](examples/value-report.md) shows
the generated fixture's failure, recovery, and refusal counts. Raw ledgers and
explicit proof exports remain more
sensitive: they preserve the evidence needed to reconstruct the run. Review
exports before sharing them. Hashes support equality checks and can reveal
low-entropy values through guessing; they are not anonymization.

The full trust boundary is defined in [SECURITY.md](../SECURITY.md).

## Measure effort without inventing it

The synthetic scenario measures elapsed automated execution and actual command
invocations. It reports manual JSON edits and the synthetic authority actions
used by the fixture. These are reproducible operation costs, not human attention
measurements. Human interaction time, novice success rate, confirmed false
positives and user-confirmed avoided incidents remain unmeasured until actual
participants provide those observations.

## Compatibility and broader claims

Historical checked runs use run-event versions 1–4 and retain their original
producer identity and semantics. The stable `v0.17.0` release writes v5 records
that bind an Accepted Subject to each result and evidence record, while readers
retain v1–v4 support. A package-number change does not rewrite old ledgers;
retain a compatible reader for historical evidence and do not relabel it as a
new Exitbind write. Public schema snapshots and frozen fixtures pin that
boundary.

One deterministic scenario supports one scoped claim. Real dogfooding across
projects, novice onboarding studies, parallel-worker stale-base handling,
automatic host observation and memory automation require their own evidence.
They are not established by this proof. See the
[measured baseline](value-baseline.md) before extrapolating to ordinary user
losses or setup effort.
