# Obtain evidence before a costly return

A clear routine task can proceed directly. For a consequential unresolved
assumption, the Lead can prepare a bounded question through the ordinary Work:

```sh
exitbind work challenge WORK --question "Can a lost reply repeat the effect?" \
  --probe-command "python3 test_behavior.py" --probe-source test_behavior.py --json
```

The explicit equivalent is `work challenge prepare WORK ...`. Its response
includes the current machine identity, a bounded evidence template and an exact
read-only validation route. Before returning, validate the completed report or
ordinary result artifact with `work challenge validate WORK --json < report.json`.
Validation gives a precise missing/invalid field error and checks that the
requirement, candidate and probe source still match. It reports shape and
identity only; the host must still observe any probe.

This read-only packet carries the current approved requirement and candidate
identity. It gives the host two separate first-pass assignments and at most one
focused exchange about a relevant disagreement. It starts no model or command.
Keep host permissions and the task's actual resource bound. Agreement does not
approve a requirement or replace a test. A useful contribution gives a claim,
source, counterexample and what would change the conclusion.

The host executes any permitted probe. A meaningful discriminator lets a
plausible wrong candidate pass the old test, rejects it for the intended behavior
with the strengthened test, and accepts a correct alternative. Compile errors,
unrelated exceptions and timeouts are inconclusive. No new information is a
valid observation; avoid prolonging a challenge merely to obtain a finding.

The packet supplies an `evidenceTemplate` for the agent's ordinary result
artifact. Put the completed template in its `earlyEvidence` field, with two
distinct contributor session references, concise claims, evidence references,
and `whatWouldChange` statements. Set the probe observation to `discriminated`,
`no_new_information` or `inconclusive`; the latter two can truthfully leave the
probe command and source empty when no probe was permitted or run. A selected
probe needs its command hash and safe current source identity. Retain exact
authorized observations in their existing artifacts. A completed return checks required fields, bounded
text, distinct session references, SHA-256 field shape and the probe command
hash before recording the return. Errors identify the missing or invalid field.
This validates the report's structure, not the truth of its claims or whether a
probe ran. A complete report whose saved Work, candidate or source no longer
matches stays historical with its original hashes; it is not refreshed in place.
These fields are reported facts, not authenticated independence or hidden
reasoning transcripts.

`work detail` shows the resulting requirement, candidate, test, observation,
actual Lead disposition and final-check links. A newer result without a report
keeps the last counterexample visible as historical, with its original hash and
a separate link to the newest candidate's final check. A missing, changed or
truncated newest artifact is unknown and never promotes an older report.
`reported_current` applies only while the declared requirement, inputs, command
and any selected probe source match; changed or unavailable inputs make the
observation historical or unknown.
This advisory link never permits skipping the frozen final check, independent
review or Lead acceptance.

When the opt-in pre-return command changes a bound input, the completed return
stays pending and the diagnostic remains passed. The refusal names the changed
binding components (such as `inputs`, `artifact`, `config`, `assignment`,
`grant_set` or `checker`) and supplies the exact saved-attempt read route. Read
the retained metadata or captured bytes there; do not repeat a committed or
uncertain diagnostic automatically.

## Select a cheap diagnostic for a new Work

An optional project command can be frozen alongside the existing final check:

When using approved setup facts, pass the same optional diagnostic flags to
`setup`; its preview/apply response carries them into the emitted begin action.
See [approved setup facts](onboarding.md#assemble-approved-facts-without-a-configuration-join).

```sh
exitbind work begin change --goal "Implement the approved behavior" \
  --check-command "./test-all.sh" --pre-return-command "./test-cheap.sh" \
  --pre-return-timeout-ms 60000 --review-policy required --detail
```

Once selected, the diagnostic must pass for that exact successful Worker
return. A known-ended failure preserves its output and current assignment;
it records no completed return or final-check credit. Repair only under the
still-valid bounded authority. A blocked report remains available. Follow the
returned stored-byte route for lost replies or incomplete execution instead
of launching the same uncertain attempt again. A genuinely defective frozen
command needs the supported Lead-owned continuation route.

After reporting the linked known-ended diagnostic as blocked, the Lead can use
`work replace-check WORK --pre-return-command COMMAND --reason TEXT
--classification defective-contract --current-binding BINDING` to preview a
successor, then execute its returned apply command. This freezes the replacement
diagnostic while preserving the final check and spent accounting. Unknown child
effects require investigation; they cannot be treated as a safe retry. The
successor needs its own current permit and evidence.

The checked boundary is the product input fingerprint, declared command and
result artifact. It is not an atomic filesystem snapshot or a claim about
uncovered services, environment or external effects. Older readers refuse an
opted-in policy they cannot enforce; historical unselected Works keep their
existing behavior.

If a pre-return was admitted but its result was not recorded, the current Work
action offers inspection and a configured Lead's read-only recovery preview.
Recovery requires a complete product-owned Linux observer proof and previously
selected owned verification preparation. Incomplete or unavailable proof stays
unresolved; the worker can report a blocker without a passing check. The Lead
must explicitly report that the execution domain ended and its effects were
reconciled. That decision records no check result: it preserves the original
unknown outcome and permits one fresh attempt of the unchanged pre-return with
fresh owned scratch. Completion still needs its captured pass and the exact
retained result. Continuation-managed Works do not use this recovery route.
The observer proof is local product-owned state; it is not authenticated against
another process with write access under the same operating-system account.

For Exitbind contributors, `scripts/ci-local.sh --pre-return` shares its stage
definition with full CI. It includes static checks and selected invariant
tests without release builds or live asset verification. Other projects choose
their own cheap command and acquire no Cargo or Clippy dependency.
