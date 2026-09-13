# Example local report

This is the aggregate output from one generated `false-completion-v1` fixture.
It contains synthetic evidence only. Reproduce it with `exitbind benchmark
--output proof-local`, then read `proof-local/report.md`. Hashes and automated
timings may vary across executions.

The two checks record the initial failure and the repaired attempt's success.
The protection count records one refused acceptance request. It does not mean
one production incident was prevented. The full exported blocked/final ledgers
and artifact fixtures support independent reconstruction of those transitions.

```text
Value proof report v1

Runs: 1

Local reports: 0 runs, 0 checks, 0 protections
Synthetic scenarios: 1 runs, 2 checks, 1 protections
Unclassified runs: 0 runs, 0 checks, 0 protections

Synthetic incident frequency, user-confirmed avoided loss, and human time are unknown.
Goals, commands, prompts, profiles, paths, and artifact content are omitted. Hashes are not anonymization.
```

See the [methodology and limits](../value-proof-methodology.md).
