<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->
# Rollout metrics

Run `rollout-metrics.py PATH` on an explicitly selected local JSONL record. The
command streams fixed-size records and prints aggregate event counts only. It
does not print or persist messages, commands, IDs, file paths, or token values.
The deduplication count covers only the configured recent-ID window; it is not a
complete count of unique commands. Malformed and oversized records are reported
separately and excluded. A token total is deliberately unavailable because
individual records may contain cumulative, overlapping, or incomparable
measurements. Compare only compatible observations and preserve their source
and measurement limits outside this command.

## Investigating a long session

For an authorized forensic question, identify the exact session, time span and
snapshot; a live file is only a point-in-time observation. Use bounded local
reads and keep raw records in their authorized private location. Examine
compaction timing, repeated operations, worker restarts and time spent on the
same decision. Separate a replayed record from a new execution by the producer's
event identity, and distinguish repeated identical commands from different
commands that re-derive the same facts. The script's bounded ID window cannot
establish a complete deduplicated history. If an event lacks a reliable identity
or a start and end cannot be paired, report that count or duration as unknown
rather than assuming a replay, a new execution or zero time.

Report counts, duration and byte measures with their source and coverage. Use
token figures only when their producer defines the scope and whether values are
per-event or cumulative. Sum only distinct nonoverlapping events; subtract
compatible cumulative snapshots only for the same defined scope without a
counter reset. Never add overlapping snapshots as independent usage. If that
basis, pricing or a comparable baseline is unavailable, say so. Label estimates
separately from measured events, and distinguish accepted outcomes, partial
artifacts and independently observed results. Avoid printing raw commands,
transcript content or identifiers into an unauthorized report.

## Optional repository packing

For a broad repository survey, an available tool such as Repomix may package a
selected local checkout or an authorized remote source for analysis. Establish
the repository and revision, allowed paths, exclusions, output location and
size before running; obtain authority for remote access or installing a tool.
Use the installed version's help for its actual options. Treat the generated
pack as potentially sensitive, inspect original source before making a claim,
and report omissions and the basis of file or token estimates. Targeted source
reading remains suitable for a narrow question and requires no packing tool.
