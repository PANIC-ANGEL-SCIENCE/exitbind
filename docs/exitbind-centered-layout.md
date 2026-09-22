# Exitbind-centered directory contract

New initialization uses Exitbind names for current control and state surfaces.
Existing `soulmate.json`, `soulmate/`, and `.soulmate/` projects remain a
readable compatibility path; their historical evidence is not relabeled.

```text
PROJECT/
├── exitbind.json
├── exitbind/
│   ├── agents/
│   ├── boundaries/
│   ├── policies/
│   └── harness/
├── .exitbind/
│   ├── runs/
│   ├── memory/
│   ├── artifacts/
│   ├── receipts/
│   ├── away/
│   └── locks/
├── .agents/
└── .claude/
```

`exitbind.json` defines the current project contract. `exitbind/` contains
reviewable profiles and boundaries. `.exitbind/` contains private runtime
evidence. Host skill directories are projections, not Exitbind identity.

New checked writes use run-event format 8 and bind the exact Accepted Subject,
tested-input identity, and current basis/review-policy extension to each result
and evidence record. Readers retain support for historical run-event formats
v1–v7 and historical `soulmate`
producer values.

Local mode keeps these control and state roots outside the product checkout.
Exitbind does not own host models, tools, permissions, process execution, or
merge authority.
