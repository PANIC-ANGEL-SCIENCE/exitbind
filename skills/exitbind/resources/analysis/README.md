<!-- exitbind-managed-skill:v1 -->
<!-- exitbind-managed-bootstrap:v1 -->
# Rollout metrics

Run `rollout-metrics.py PATH` on an explicitly selected local JSONL record. It
opens a regular file without following a final symlink, bounds each record, and
reads no more than its initial size. It prints aggregates, never messages,
commands, IDs, paths, hashes, or token values. A changed size or modification
time during the read is reported; unchanged metadata does not prove an inactive
source. Malformed and oversized records are reported separately and excluded.

Command aggregates cover completed `CommandExecution` records with distinct,
tracked IDs. The existing `--dedupe-window` option now caps an exact ID set
without eviction: at most 4096 IDs, each at most 256 UTF-8 bytes. Missing,
invalid, and over-cap IDs are counted as untracked records and excluded from
command aggregates; `dedupe_complete` identifies that limit. The legacy
`command_replays_in_window` field counts replays of retained IDs. Hour counts
use validated UTC timestamps with at most 744 distinct hour bins; missing and
over-cap hours are counted separately. Check `hourly_complete` before treating
the histogram as complete.

The fixed command families classify only the first executable token; shell
wrappers, compound commands and unrecognized tools may land in `other` or a
wrapper family. Exact repetition counts require identical full command text for
distinct tracked IDs, not merely similar work. At most 4096 command strings of
at most 1024 UTF-8 bytes are retained temporarily, plus the ID set, hour bins,
one bounded record and runtime overhead. Missing or longer commands are counted
as untracked; `command_patterns_complete` states the coverage. No retained text
or identifiers are written or printed.

Command duration and token totals are unavailable: the supported observations
do not establish paired start/end measurements or nonoverlapping token usage.
Compare only compatible observations and preserve their source and measurement
limits outside this command.

## Investigating a long session

For an authorized forensic question, identify the exact session, time span and
snapshot; a live file is only a point-in-time observation. Use bounded local
reads and keep raw records in their authorized private location. Examine
compaction timing, repeated operations, worker restarts and time spent on the
same decision. Separate a replayed record from a new execution by the producer's
event identity, and distinguish repeated identical commands from different
commands that re-derive the same facts. The script's bounded ID set cannot
establish a complete deduplicated history after coverage is lost. If an event
lacks a reliable identity or a start and end cannot be paired, report that count
or duration as unknown rather than assuming a replay, a new execution or zero
time.

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
