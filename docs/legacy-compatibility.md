# Legacy compatibility

Exitbind is the renamed continuation of Soulmate. A new install never meets that
name; this document exists for the cases that do — an old project, an old
ledger, an old executable, or a rollback.

The rule behind every entry below: compatibility reads the old world, and never
relabels its evidence as new evidence. Active Soulmate distribution ended in
`v0.25.0`; historical releases and records remain available.
Original releases and unmodified records are retained in the private archive.
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
| `systems.soulmate/` | `systems.exitbind` plugin identifier | Retained Soulmate metadata uses neutral public identifiers; reinstall the current Exitbind plugin to replace an old registration |

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

The schema files `schema/soulmate.schema.json`, `schema/run-event-v3.schema.json`,
and `schema/run-event-v4.schema.json` describe historical formats. Their format
constraints remain unchanged. Public schema-address annotations and maintainer
identifiers were updated during the organization import; the exact original
schema bytes remain in the private archive. Existing ledgers and their hashes
are never rewritten.

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
- The `systems.soulmate` plugin files retain historical Soulmate metadata under
  a neutral directory name. They do not alias an old host registration. Current
  packaging contains only the Exitbind plugin; reinstall it through the
  `exitbind-plugins` marketplace when replacing an earlier registration.
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

## Reader inventory

Every source file that reads a historical form appears below exactly as
`tooling/repository/checks/check_legacy_inventory.py` finds it: each file whose
code names the old product, and each file whose code compares a recorded format
version with a number from 1 to 7. The check fails when a found file is missing
from this table, when a listed file no longer exists, or when a form marked
`retired` is read again anywhere in the source.

"Live" means a file of that form exists on the owner-provided Linux runner, found
by a full scan of its home and data volume; "fixture" means a frozen file under
`tests/fixtures/`. A reader is retired only when its form has neither and no
current code writes it.

New starts write run-event version 8 only. The current binary still appends to
an open historical ledger in that ledger's own version, because a ledger never
mixes versions; that is the "append" write below.

| Reader | Old form | Last write | Live file or fixture | Pinning test | Decision |
| --- | --- | --- | --- | --- | --- |
| `src/run/state/validation.rs`, `src/run/reducer.rs`, `src/run/state/unknown_fields.rs` | run-event v1 (no producer, or `soulmate` producer) | new start: superseded by v2 and v3; append: current binary | live (7 ledgers); three v1 run fixtures | `tests/rust_parity.rs`, `tests/value_replay.rs` | keep: live and fixture |
| `src/run/reducer.rs`, `src/run/state/unknown_fields.rs`, `src/run/state/validation.rs` | run-event v2 (receipt-referenced run) | new start: superseded by v3; append: current binary | one v2 run fixture | `tests/rust_parity.rs`, `tests/fresh_user_surface.rs` | keep: fixture |
| `src/run/mod.rs`, `src/run/gate/value/validation.rs`, `src/run/state/validation.rs`, `src/run/state/unknown_fields.rs` | run-event v3 (checked run, caller-reported check) | new start: superseded by v4; append: current binary writes the v3 check shape | live (10 ledgers); fixture `run-v3.jsonl` | `tests/value_replay.rs`, `tests/compatibility_matrix/projection_compatibility.rs`, `tests/exit_transition_conformance/historical_compatibility.rs` | keep: live and fixture |
| `src/run/human.rs`, `src/run/gate/value/human.rs`, `src/run/gate/value/validation.rs`, `src/run/observed_check.rs`, `src/run/mod.rs` | run-event v4 (observed or reported check; last `soulmate` producer) | new start: superseded by v5; append: current binary | live (11 ledgers) | `tests/value_reconstruction/bundle_helpers.rs`, `tests/value_reconstruction/context.rs` | keep: live |
| `src/producer.rs`, `src/run/gate/exit.rs`, `src/run/lifecycle.rs`, `src/run/submission.rs`, `src/run/view.rs`, `src/run/state/apply.rs`, `src/run/observed_check.rs` | run-event v5 (Accepted Subject bindings; first `exitbind` producer) | new start: superseded by v6; append: current binary | live (58 ledgers) | `tests/value_reconstruction/context.rs`, `tests/context_v022/context_projection.rs` | keep: live |
| `src/run/disposition.rs`, `src/run/result_reference.rs`, `src/run/state/governor_validation.rs`, `src/run/gate/exit.rs`, `src/run/gate/value/validation.rs`, `src/run/lifecycle.rs`, `src/run/submission.rs`, `src/run/view.rs` | run-event v6 (tested-input bindings) | new start: superseded by v7; append: current binary | live (33 ledgers) | `tests/compatibility_matrix.rs` (`producer-current-v6-write`) | keep: live |
| `src/run/state/validation.rs`, `src/run/reducer.rs`, `src/run/state/unknown_fields.rs` | run-event v7 (fallback-provenance submit shape) | new start: superseded by v8; append: current binary | live (1 ledger); no fixture | `tests/docs_workflow.rs` (reader range text) | keep: live |
| `src/producer.rs`, `src/run/state/validation.rs` | producer `soulmate` on v1–v4 events | append to an open v1–v4 ledger | live (v1, v3, v4 ledgers); fixtures above | `tests/compatibility_matrix.rs`, `tests/rust_parity.rs` | keep: live and fixture |
| `src/project/layout_types.rs`, `src/evidence/harness.rs` | `soulmate.json`, `soulmate/` control tree, `.soulmate/` state tree | never by a current install | live (298 paths) | `tests/layout_migration.rs`, `tests/project_capabilities.rs`, `tests/path_compatibility.rs` | keep: live |
| `src/memory/contextual.rs` | `.soulmate/artifacts/memory-choices` contextual source | never by a current install | live `.soulmate/` trees | `tests/task_local_memory.rs` | keep: live |
| `src/run/inputs.rs`, `src/run/verification_preparation.rs`, `src/project/continuation/model.rs`, `src/work/file_effect.rs` | `.soulmate/` as a protected, never-hashed directory | never by a current install | live `.soulmate/` trees | `tests/harness_receipt.rs`, `tests/project_capabilities.rs` | keep: live (protection, not a reader) |
| `src/host/hooks.rs`, `src/host/settings.rs`, `src/compatibility/mod.rs` | hook token `soulmate-hook-v1`; `soulmate hook-run` host record | token: emitted today; record: never by a current install | emitted | `tests/rust_parity.rs`, `tests/host_bridge.rs` | keep: still written; criterion in [the hook protocol token](#the-hook-protocol-token) |
| `src/main.rs`, `src/compatibility/mod.rs` | invocation as `soulmate` | never | not a file | `tests/build_identity.rs` | keep: refusal guard, not a reader |
| `src/project/binding_location.rs`, `src/host/assignment_evidence.rs`, `src/distribution/update.rs`, `src/host/away.rs` | `SOULMATE_*` environment inputs and the `soulmate` bindings directory | never by a current install | environment, not a file | `tests/portability.rs`, `tests/host_bridge.rs`, `tests/legacy_installer_bridge.rs` | keep: outside the on-disk slice |
| `src/project/onboarding.rs` | `schema/soulmate.schema.json` | historical schema bytes | schema file | `tests/setup.rs`, `tests/release_integrity/release_policy.rs` | keep: published schema |
| `src/session_goal/continuity/capture/same_host.rs` | the `your.soulmate/lenses/` directory of the user-owned agent convention | current: the owner's lens files | lens files under the ControlRoot | `tests/native_continuity/same_host_lens.rs` | keep: a current directory name, not a historical form |
| `src/cli/help.rs`, `src/cli/mod.rs`, `src/envelope.rs`, `src/host/runtime.rs`, `src/host/runtime/context.rs`, `src/host/runtime/project.rs`, `src/presentation/run.rs`, `src/project/commands.rs`, `src/project/skills.rs`, `src/value_benchmark.rs`, `src/value_benchmark/invocation.rs`, `src/value_benchmark/result.rs` | branches for the retired Soulmate surface, selected only when the profile is not Exitbind | never: the profile has one surface | some read live forms (`soulmate.json` in hook routing) | `src/compatibility/mod.rs` unit tests | keep in this slice: unreachable; retire as one class with every `exitbind_surface()` branch |
| `src/run/error.rs` | `SOULMATE_DRIFT:` in-process drift channel | never: no code writes it | none: never persisted | `src/run/error.rs` unit test | retired |
| `src/main.rs`, `src/cli/mod.rs` | `SOULMATE_JSON:` in-process machine channel | Soulmate surface branch only | none: never persisted | `src/cli/mod.rs` unit test | retired |
| `src/evidence/receipt.rs`, `src/memory/choices.rs`, `src/memory/contextual.rs`, `src/memory/revalidation.rs` | receipt, memory choice and contextual policy versions 1 and 2 | both versions written today | written | `tests/task_local_memory.rs`, `tests/harness_receipt.rs` | keep: current forms, not historical |
| `src/config/boundary.rs`, `src/config/mod.rs`, `src/evidence/harness.rs`, `src/memory/lessons/input.rs`, `src/memory/policy_correction.rs`, `src/project/architecture/validation.rs`, `src/project/capabilities.rs`, `src/project/continuation/amendment.rs`, `src/project/continuation/journal.rs`, `src/project/continuation/model.rs`, `src/project/specialists.rs`, `src/run/check_recovery.rs`, `src/run/governor_recovery.rs`, `src/run/guidance_continuity.rs`, `src/run/ledger/claim.rs`, `src/run/pre_return/execution.rs`, `src/run/state.rs`, `src/run/verification_preparation.rs`, `src/session_goal/display.rs`, `src/session_goal/requirements/model.rs`, `src/session_goal/usage/ingest.rs`, `src/work/file_effect_recovery.rs`, `src/work/managed_edit.rs`, `src/work/managed_edit/state.rs`, `src/work/native_action/continuation.rs`, `src/work/native_action/continuation/failure.rs`, `src/work/native_action/journal.rs`, `src/work/native_action/recovery.rs` | single-version formats checked for an exact version | current | written | each format's own tests | keep: exact match, no older form read |
