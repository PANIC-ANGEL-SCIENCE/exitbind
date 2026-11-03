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
