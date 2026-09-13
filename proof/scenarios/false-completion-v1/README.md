# False-completion v1

This deterministic, token-free scenario supports one narrow sentence:

> In checked runs, Exitbind refuses acceptance when the configured check result is missing or reports failure for the current worker artifact.

It creates a disposable portable fixture, runs a competent legacy workflow with
four distinct generic artifacts, and observes the same deterministic check
return a nonzero exit code. The legacy workflow can still reach acceptance; its
nonzero result remains an observed baseline fact. The protected v3 workflow
binds the reported check to the current worker submission, records one factual
protection event when lead acceptance is attempted, preserves the blocked
ledger, then recovers through a fresh worker attempt after the fixture is
repaired.

The scenario verifies that a failed check blocks canonical acceptance, that a
passing check alone and reviewer approval alone do not accept a run, and that
the configured lead can accept only after the repaired attempt is checked and
reviewed. It also verifies that the previous attempt bytes and hashes remain
available and that the generated report separates synthetic evidence from
local caller reports while omitting sensitive goals, commands, prompts,
profiles, transcripts, paths, and artifact contents.

Run it with the installed binary:

```sh
exitbind benchmark
```

From a checkout, the suite entry point is:

```sh
EXITBIND_BIN=target/debug/exitbind ./scripts/run-value-proof-suite.sh
```

The stable expected machine result is in [expected.json](expected.json).
Timing values, generated run identifiers, event hashes, and local paths are
deliberately excluded from that fixture. Human interaction time is unmeasured;
the synthetic command count and automated elapsed time are reproducibility
measurements rather than real-user effort or avoided-loss evidence.

An explicit bundle includes `ledgers/baseline.jsonl`,
`ledgers/blocked.jsonl`, `ledgers/final.jsonl`, the generated fixtures under
`fixtures/`, all three versioned schemas under `schema/`, and a sorted
`hash-manifest.json`. Those ledger, fixture, schema, and manifest files are
sufficient to reconstruct the stable assertions without using the result
summary, report summary, or an agent transcript.
