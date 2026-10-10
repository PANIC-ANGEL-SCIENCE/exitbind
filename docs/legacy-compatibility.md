# Legacy compatibility

Exitbind is the renamed continuation of Soulmate. A new install never meets that
name; this document exists for the cases that do — an old project, an old
ledger, an old executable, or a rollback.

The rule behind every entry below: compatibility reads the old world, and never
relabels its evidence as new evidence. Active Soulmate distribution ended in
`v0.25.0`; historical releases and records remain available.
The current release also retires its old executable profile and updater route.

## Rename history

The rename kept the existing version and repository history rather than
resetting to `0.0.1`. The published tag line, the release assets, and the
run-event format numbers continue across it. See the
[public format map](../CHANGELOG.md#public-tags-and-format-readers) before
choosing a rollback or release channel.

For current executable changes and rollback limits, see
[upgrade and rollback](upgrade-and-rollback.md).

## Old command, config, and state paths

| Historical surface | Current surface | Status |
| --- | --- | --- |
| `soulmate` executable | `exitbind` | Not built or installed by default at `v0.25.0`; the current release no longer builds or selects the old profile, and refuses copy or symlink invocations before CLI effects |
| `soulmate.json` | `exitbind.json` | Read for existing projects; never created by a current install |
| `.soulmate/` state | `.exitbind/` | Read for existing projects; never created by a current install |
| `soulmate/` control tree | `exitbind/` | Read for existing projects; never created by a current install |
| `.agents/skills/soulmate/`, `.claude/skills/soulmate/` | `.../skills/exitbind/` | Historical project files remain readable; current setup installs only Exitbind guidance |
| `SOULMATE_*` environment controls | `EXITBIND_*` | Historical installer compatibility inputs remain; the active updater follows only the Exitbind profile |
| `systems.soulmate/` | `systems.exitbind/` | Retained for already-published plugin consumers; not referenced by current onboarding |

A current Exitbind install creates none of the historical spellings. That is a
tested property, not an intention: see `tests/fresh_user_surface.rs`.

## Historical producer and schema guarantees

Run events written by Soulmate carry `producer.name = "soulmate"` and run-event
versions 1 through 4. Exitbind reads them under that identity and leaves the
recorded producer, schema version, and hashes exactly as written. Reading an old
ledger never rewrites it, and a successor run records the current identity
without restating the old one as its own.

Checked runs written by Soulmate use run-event version 3; new Exitbind starts
use version 8, while current readers retain historical versions 1 through 7.
`REFERENCE.md` holds the exact reader and writer matrix.

The immutable schema files `schema/soulmate.schema.json`,
`schema/run-event-v3.schema.json`, and `schema/run-event-v4.schema.json` describe
historical formats and are never edited.

## The hook protocol token

A binary that manages host hooks asks the CLI on `PATH` for its hook protocol
token and refuses to write records it could not have produced. Every published
release compares that token exactly against `soulmate-hook-v1`, so the current
surface keeps emitting the historical spelling: renaming it now would make an
already-published binary reject a newer CLI found on `PATH`.

The current binary already accepts `exitbind-hook-v1` as well. Retirement
criterion: once no supported release older than that change can be the binary
that manages hooks, the emitted token can become `exitbind-hook-v1` and the
historical token moves to read-only acceptance.

Session-hook records are separate from the token. A current install writes only
`exitbind hook-run` records, and installing on the Exitbind surface retires a
superseded `soulmate hook-run` record it recognizes exactly, leaving every other
hook alone.

## Work handles

Work and assignment handles carry the `sma_` prefix. It is an opaque identifier
derived from the previous name, parsed by every published release, and is not
rendered as product identity anywhere in the interface.

## Migration behavior

- An existing project keeps its historical paths; nothing is moved without an
  explicit migration command.
- `exitbind` reads a historical project. An executable from a historical tag
  retains its original layout; the `v0.25.0` package does not ship it.
- Old and new state never mix: a project answers to one configuration, and a new
  run records the current identity only.

## Retained compatibility boundaries

- No current source build, install or release package provides a `soulmate`
  executable or selects a Soulmate updater profile. Frozen historical fixtures
  preserve old producer, path and installer records without building that
  executable.
- The `systems.soulmate` plugin files remain in source for already
  published consumers, but current packaging contains only the Exitbind plugin.
- The historical hook protocol token remains under the criterion above.

Historical readers and published evidence remain independent of active
Soulmate distribution.

## Working selector and active Work

Updating the executable or managed guidance selects a reader for future work.
Keep each active Work's original immutable governor, configuration and ledger.
Do not substitute a new observer into an admitted check or add current markers
to old history. The current reader keeps markerless semantics unchanged; it
cannot retroactively repair or accept old held Work.

A current marked carried re-plan may record inputs from its permitted edit.
Earlier readers that reject that transition are a compatibility limit: retain
the current reader for that Work. Rollback restores retained executable and
managed guidance bytes for their supported history; it does not downgrade the
ledger or transfer acceptance to changed inputs. Preserve custom profiles and
settings.
