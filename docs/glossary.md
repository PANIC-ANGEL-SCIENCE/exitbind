# Translate the record into your existing workflow

You can ask your host one question: **what still needs doing before I can
accept this change?** These terms explain the references behind its answer;
they are not extra steps to memorize.

| Exitbind term | Familiar equivalent | Distinction to keep |
| --- | --- | --- |
| Assignment / brief | A bounded subagent task | Reading it does not spawn an agent. |
| Worker submission | The worker's result document | A completion claim is not a passing test. |
| Check report | The command and exit the host reports | Exitbind does not execute or authenticate the test. |
| Reviewer finding | A separate review of the change | Approval still needs a lead decision. |
| Lead acceptance | The task owner's recorded decision | It neither merges Git changes nor proves the code correct. |
| Artifact | A referenced result document | Its recorded bytes must remain unchanged. Product files can change in later attempts. |
| Ledger / run | The task's inspectable sequence of records | It is not a recovered host conversation. |
| Rework | Another attempt within the frozen task | Use fresh result documents, checks, and review. |
| Wrong door | Test or review evidence from an earlier version of the change | Product name only: the CLI reports it as `EXIT BLOCKED` or `EXIT REFUSED` with a reason code, and never reuses that evidence for the current result. |
| Observed check | v4–v6 source evidence from executing the frozen command in ProductRoot | It records an exit or signal locally; it still needs review and acceptance. |
| Reported check | Caller-supplied result bound to the frozen command | It is the only check route in v3 and one permitted route in v4–v6; it records what was reported, not proof that the caller ran the command. |
| Supersede | An explicit successor after governing inputs change | The predecessor remains inspectable; final runs stay final. |
| Declared boundary | The task's stated file/command limits | The host owns execution permissions; this is not a sandbox. |
| ControlRoot | Configuration and profiles directory | Portable mode puts it in the project; local mode keeps it elsewhere. |
| ProductRoot | The project being changed | It is distinct from private records. |
| StateRoot | Private records directory | An ignore rule is not access control. |
| Receipt | A snapshot of selected configuration references | It does not prove actions were performed. |
| Accepted Subject | Exact goal, plan, configuration, and subject hash for a checked run | A result or evidence record bound to a different subject is stale and cannot qualify the run. |
| Exitbind progress | Weighted progress toward `EXIT READY` | It is overall system progress; Holytail preservation detail is separate and exceptional. |

[Do the next change](first-checked-run.md#use-your-own-project) ·
[Repair or resume](repair-a-run.md) ·
[Optional surfaces](optional-surfaces.md).
The [reference](../REFERENCE.md) remains canonical for flags and formats.
