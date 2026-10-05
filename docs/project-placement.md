# Project placement on managed hosts

Use `exitbind init --mode auto --root SOURCE` when source and durable state need
separate locations. If no mode was selected and the source or preferred private
state directory is read-only, initialization selects this automatic path.
Source stays in place; installation/package files and Git metadata need no writes.

Automatic bootstrap uses Git's canonical worktree root when available, including
linked worktrees and gitfiles. Without Git it uses the selected source directory.
It creates private owned control and state directories under `XDG_STATE_HOME`,
then the ordinary user state directory if the preferred location is unavailable.
It records one existing local-project binding and a deterministic source-based
project ID. State/runtime placement never changes that identity.
An existing fallback binding registry remains authoritative when the preferred
state location becomes writable later. Both default registries remain discoverable. Duplicate bindings for one project
ID require an explicit authoritative registry choice; bootstrap never merges them.

The existing explicit local mode and its `--project-id`, `--control-root` and
`--state-root` choices remain available. Explicit automatic roots take precedence.
Portable mode keeps its existing project-local layout. Automatic bootstrap refuses
an already-configured source instead of forking its authority or moving
existing ledgers. Inspect its current configuration before selecting a migration.

`exitbind project context --json` discovers the same configuration from nested
source directories and reports `placement`: source root, Git root and metadata
locations, topology, control/state/runtime roots and observed capabilities. It
uses canonical Git interfaces and does not assume `.git` is a writable directory.
A no-Git or wrapped workspace receives an explicit unavailable Git classification.
An ordinary portable configuration at the user's home directory is not inherited
by unrelated no-Git child workspaces. Use its explicit configuration or local
project binding when that home directory itself is the selected project.
Read-only context and native context propagation do not chmod binding metadata.

A read-only source can support context inspection with writable external state.
A new file effect on that source is refused as an unsupported mutation before
admission; state placement is reported separately. When no safe writable owned
storage exists, bootstrap returns one actionable `unsupported storage` refusal.
It never copies the repository or weakens permissions to manufacture support.
