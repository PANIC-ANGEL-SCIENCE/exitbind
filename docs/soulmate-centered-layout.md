# Historical Soulmate-centered directory contract

Status: historical compatibility architecture accepted during the Soulmate
era. Fresh Exitbind projects use the
[Exitbind-centered layout](exitbind-centered-layout.md). Existing Soulmate
projects and persisted evidence are not rewritten implicitly.

## Historical ownership model

```text
PROJECT/
├── soulmate.json
├── soulmate/
│   ├── agents/
│   ├── boundaries/
│   ├── policies/
│   └── harness/
├── .soulmate/
│   ├── runs/
│   ├── memory/
│   ├── artifacts/
│   ├── receipts/
│   ├── away/
│   └── locks/
├── .agents/
├── .codex/
└── .claude/
```

The directories express authority, not containment:

- `soulmate.json` defines what the bounded agent system is.
- `soulmate/` owns public, reviewable semantics: who, why, and what may be
  requested. Canonical profiles belong in `soulmate/agents/`.
- `.soulmate/` owns private mutable state and evidence: what happened.
- `.agents/` owns optional distribution resources. A Soulmate skill there is a
  projection, not canonical Soulmate identity.
- `.codex/` and `.claude/` are disposable host/execution projections.

Public control material and private evidence must not be mixed. Hash-verified
state remains evidence and does not gain instruction authority from its path.

## Exitbind compatibility

Legacy `soulmate init` projects and `soulmate profile import` operations use
`soulmate/agents/`. Existing configurations may continue to name
`.agents/profiles/` or another confined ControlRoot-relative profile path.
Soulmate never moves those files implicitly or reinterprets old ledger and
receipt bytes. `soulmate migrate layout --config CONFIG` prints the exact
legacy-profile plan without mutation; repeating it with `--apply` copies the
reviewed bytes, atomically repoints the configuration, and removes only the
configured legacy sources. Symlinks, target collisions, and tracked or staged
Git paths fail closed.

New initialization prepares the complete public `soulmate/` and private
`.soulmate/` directory contract shown above. Canonical run paths use versioned,
path-derived transient lock names beneath `.soulmate/locks/`; legacy ledger
paths keep their adjacent lock convention. Receipt v2 records either the
canonical manifest path or the exact legacy root path, so existing receipts
remain verifiable without byte rewriting.

For an existing project, `soulmate migrate paths --config CONFIG` reports the
directories and optional legacy-manifest copy; `--apply` creates them and keeps
the legacy manifest bytes in place. It never moves ledgers or receipts.

`.agents/skills/`, dotagents lockfiles, and host-native files retain their
existing ecosystem locations. Distribution and execution remain replaceable
planes rather than being absorbed into Soulmate.
