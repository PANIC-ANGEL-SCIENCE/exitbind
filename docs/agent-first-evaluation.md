# Agent-first control-surface evaluation

This deterministic comparison exercises the same one-worker checked `change`
workflow through the advanced low-level `run` interface and the high-level
`work` façade. Both paths produce the same ordered strict-core event trace:
start, lead scope, worker completion, observed passing check, reviewer approval,
and lead acceptance.

The executable evidence is
`matched_workflows_measure_protocol_transport_without_product_overclaim` in
`tests/agent_first_control.rs`.

| Measured item | Low-level `run` path | High-level `work` path |
| --- | ---: | ---: |
| Exitbind process calls | 6 | 6 |
| Raw protocol identifier occurrences in command inputs | 11 | 0 |
| Caller-selected bookkeeping inputs | 10 | 0 |
| Observed protocol errors or retries | 0 | 0 |
| Operator interventions | 0 | 0 |
| Human protocol-state transfers | 0 | 0 |

The raw-identifier count includes each command-input occurrence of a ledger
path, artifact path, or event target. The low-level path supplies the ledger on
six calls, four artifact paths, and one worker event target. Opaque `smw_` and
`sma_` handles are not raw ledger, event, or artifact identities.

Caller-selected bookkeeping inputs count one ledger name, four artifact-path
allocations, four artifact-root selections, and one event-target selection on
the low-level path. The façade owns those inputs internally. The task goal and
authorized check are meaningful workflow inputs, so neither is counted as
protocol bookkeeping.

This is a scripted local integration test, not a human or native-agent study.
The zero error, retry, intervention, and human-transfer observations describe
only these successful scripted runs. They do not establish lower token use,
faster completion, better code, human adoption, universal host behavior, or a
recovery-path improvement.
