# Add an optional surface only when work needs it

Exitbind does not own model execution, permissions, or conversation continuity;
the host does. None of these options is required for the first checked change.

| Recurring need | Optional surface |
| --- | --- |
| Inspect which configuration was selected | [Receipts and integrations](../REFERENCE.md#advanced-integrations) |
| Present bounded context at host lifecycle events | [Explicit host hooks](../REFERENCE.md#optional-codex-and-claude-hooks) |
| Retain approved role-scoped references across tasks | [Memory governance](../REFERENCE.md#memory-governance) |
| Continue an explicitly authorized unattended assignment | [Away execution](codex-tmux-away.md) |

Keep the existing host workflow and enable only the surface that removes a
recurring step you can identify. Hook installation is not activation or model
compliance; receipts are not proof of execution. Memory remains opt-in,
reviewable, expiring, and revocable.

Use `exitbind help advanced` for the command map and
[removal](../REFERENCE.md#removal) to inspect what remains when disabling a
surface. [Return to the next change](first-checked-run.md#use-your-own-project).
