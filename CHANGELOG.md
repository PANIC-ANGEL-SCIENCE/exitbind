# Changelog

## 0.31.24

Release: https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/tag/v0.31.24 — the tag sits on this entry's release commit, and the page lists its assets once they are published.

Install compatible hook and governor binaries together. New Works that select
pre-return carry `preReturnAdmissionProtocol` and record `pre_return_admitted`
before their checker starts. Earlier readers refuse this extension and the
`pre_return_reconciled` recovery action. Keep each existing Work's original
binary; never edit a ledger or downgrade a newly marked Work to bypass a
refusal. Markerless historical and successor runs retain conservative lookup,
including refusal when a damaged record's ownership cannot be established.
Admission advances the audit head without worker-result, final-check, grant or
acceptance credit. Saved native results and bound returns validate that exact
owned audit suffix without ignoring changed inputs or foreign events.

- Goal commands share one dispatcher/admission type, with public positive,
  foreign-owner and no-write tests instead of a lexical applying-command list.
- New maintenance pre-return checks use the existing affected-target relation
  and scoped lint. Unknown dependencies retain conservative coverage; full
  integration, security, packaging and installer checks stay at qualification.
- Owned interrupted pre-return recovery preserves unknown outcomes, exact held
  results, process ownership and the single explicitly reconciled retry. A
  failed linked retry offers inspection or a truthful blocked report.
- Separate newly marked Works isolate unrelated damaged admissions. Relevant
  missing, moved or forged evidence and unresolved cross-producer effects refuse.
- Configured native recipients agree across full, current and action views.
  Preparation failures identify preparation as the blocker.
- Runtime historical-reader fixtures replace repository compatibility/source
  inventories. Repository byte ceilings, gate/failure bookkeeping and historical
  test-source hash adaptation records are retired; real reader, temporary-file,
  failed-evidence, privacy and identity behavior remains checked.
- Plugin validation derives its current version from Cargo. Existing stable,
  paired, provenance, installer and package boundaries remain supported.

## 0.31.23

Release: https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/tag/v0.31.23 — the tag sits on this entry's release commit, and the page lists its assets once they are published.

Install the hook binary and the governor together, and check that
`exitbind version` on PATH (the hook binary) prints the governor's version.
There is no rollback to 0.31.22 once 0.31.23 has written these records.
0.31.23 records every check admission's
`observer`, with the observer's `pid` and `startTimeTicks`, and a failure
that kept an owned checker group records `ownedProcessGroup` and, when the
capsule could read it, the leader's `ownedProcessStart`. An environmental
Lead decision that rests on an owned group already observed ended records
that fact as `ownedProcessState` on its submit event (the ledger field
`submit.ownedProcessState`), so a later ledger read validates the decision
without inspecting host processes. A 0.31.22 reader
admits no such field and no ledger version above 8, and it cannot tell a newer
Exitbind's ledger from a corrupt one: it reports the check ledger as
unreadable (`corrupt_ledger`) and offers inspection. Once 0.31.23 appends a
change record (`lead_session_goal_delta`) to a Goal journal, 0.31.22 refuses
that journal with an event hash mismatch. This is not corruption: do not
delete or edit the journal; upgrade every binary that reads it, and do not
downgrade.

- A Goal journal stores each revision as a change record against the record
  before it, under the same sealed hash chain, and every journal written
  before still reads unchanged; recovery reads a journal as large as R55's
  (about 18 MB in 114 records) instead of refusing it.
- A compatibility-marker table lists every in-band protocol marker and every
  record field the current release adds, with its writer, reader and
  retirement condition; a static check without a build fails on an unlisted
  marker or field, and `release_integrity` cross-checks the table's
  record-field rows.
- `check`, `setup --apply`, `goal prepare` and `work begin` state one set of
  environment readiness facts (tested-input breadth, Git worktree and
  baseline, whether the frozen checker can run, build target, host write
  posture) before their first write; only a checker file that exists but
  cannot run refuses, with one route.
- A native start that recorded no provider thread and no effect reads
  `not_started` with a typed cause and one printed route that relaunches the
  same assignment with its recorded launch values; a host write denial
  (`HostPermissionDenied`) leaves only the sandbox for the human to choose; a
  check whose observer died closes as failed onto the existing quarantine
  route; and a timed-out check whose owned process group has ended reaches
  the environmental re-run, while live members keep every forward route
  closed and are named.
- Every `ci-local` mode starts with a no-build repository-rule guard as
  stage 0 (size budgets, path rendering, product surface, release
  references, public history and commit identity) that fails in seconds
  before any cargo stage; it absorbs the history, path-rendering,
  product-surface and release-refs stages, and the gate inventory's failure
  ledger records the failures each gate really caught.

## 0.31.22

Release: https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/tag/v0.31.22 — the tag sits on this entry's release commit, and the page lists its assets once they are published.

Install the hook binary and the governor together, and check that
`exitbind version` on PATH (the hook binary) prints the governor's version
before relying on saved Lead decisions, adopted Goals or journey members.
There is no rollback to 0.31.21 once 0.31.22 has written these fields. Every
Work 0.31.22 starts carries `start.observationNoveltyProtocol`, so 0.31.21
refuses every such Work (`invalid run ledger line 1: unknown field
'observationNoveltyProtocol'`); it also refuses a Lead act's
`submit.reason`. A 0.31.21 hook that reads a session recovery hint carrying
`latestCorrectionWorks` drops the saved Lead decisions the next time it saves
a step; one carrying `recordedBySessionKey` it reports unusable, and its
record writes fail until 0.31.22 rewrites the hint. After `goal adopt`,
0.31.21 refuses the adopting session's owner record carrying
`journalSessionKey` and does not carry `sessionAdoptions` into the Goal
revisions it writes. A configuration replaced by 0.31.21, or by a writer
outside the owner's native session, keeps no stamped copy, so journey
members stamped with the replaced bytes stay refused until those bytes are
supplied with
`memory revalidate LEAD --from-config PREVIOUS_CONFIG --reason TEXT --apply`.

- The architecture budget covers every file under `src`, `tests` and
  `tests/fixtures`; files already over 32 KiB keep a ceiling that only falls,
  and near-limit sources move one responsibility each into its own module.
- A recipient's name, profile and phase guidance carry no lesson authority;
  `memory revalidate` takes several ledgers, checks them all under one lock
  before any append, and the configuration-changed refusal prints one route
  for every ledger stamped with the same kept copy.
- A passing observed check on a new worker result is new governor
  information; the same pass on the same result and a repeated failure still
  count toward their limits.
- Every hook event resolves the project its session's native Goal belongs
  to, verified by owner identity and project ID, never by path alone, and
  says which configuration it used.
- For a Work held in the current session, `work next` prints a
  `work child prepare` route with lens files and no `--context`; a same-host
  child whose binding stops verifying is failed with its message kept, not
  lost.
- `work disposition --decision environmental --reason TEXT` re-runs a failed
  check on the same result; the failure stays in the ledger and still counts
  toward the repeated-failure limit.
- Every replacement of `exitbind.json`, including `profile import` and
  `migrate layout --apply`, goes through one writer, which first keeps the
  bytes it replaces when a lesson ledger, or a journey member of the
  owner's Goal written in the owner's native session, carries their hash.
- `goal adopt --session-key KEY` lets a new session read and write the
  project's one open Goal, printed as one route wherever the Goal reads
  unavailable.
- `session recovery record --step TEXT` and `--decision TEXT` save a Lead
  pause through printed routes that `goal status`, `work resume` and
  `session recovery show` offer; a second `work check` while an observation
  runs is refused naming it.
- Every printed route is a typed value from one builder and runs as printed,
  including permit, continuation, reconcile and propose routes; a registry
  test fails on a route field not rendered from it.
- A Lead's `--reason` on `work return`, `work fill`, `work act` and
  `run submit` is recorded on the submit event (ledger version 8, at most
  1024 bytes), together with the saved Lead decisions bound to that Work,
  which the act then spends.
- Every test takes its temp root from one guard that removes it on success
  and names it on failure; `test-affected.sh` keeps logs only for failing
  targets.
- Inventory oracles read the product with one shared Rust source scanner and
  the CLI's own tables; the gate (26) and legacy-reader inventories run on
  every platform, and an expired provisional gate is refused at release.
- `work child recover` without `--context` recovers a failed same-host
  child; a refusal a retained child causes names it and prints its recover
  line; session record lock refusals name the session record.
- A journey member stays integrable across a configuration change that
  moves no authority, proven by the kept copy of the bytes it was stamped
  with; without that copy `goal prepare`, `work begin --journey-integration`
  and `goal status` print one route naming the members.

## 0.31.21

Published stable release; the `v0.31.21` tag and release assets are available.

- `work detail WORK --view action` carries the phase guidance and project
  lessons delivered to the current recipient, each with its read route, and
  counts omitted items with their reason.
- Printed routes, including the follow-up commands refusals and hints name,
  come from one builder that names the executable and `--config`, with
  registered `<NAME>` placeholders; `work fill WORK --current-binding B
  --choice LABEL` runs a printed route with its placeholders filled, unedited.
- `goal prepare --scope integration` over a conversation journey whose
  members are all accepted prints `--journey-integration` in its begin route;
  otherwise `journeyIntegrationOmitted` states why the flag is absent.
- `setup --task-card --first-work IDS` prints `goal prepare` and `work begin`
  for those rows only and keeps every row registered.
- Under a session Goal owned by another project, `goal` mutations refuse
  before their first write (naming `--project` where they accept it), and
  `setup` selects the project journal and says so in `scopeReason`.
- A bound input that changed reads as one typed refusal with the bound and
  current sha256 and the exact next command; an accepted Work whose input
  changed reads accepted at binding.
- `memory revalidate` accepts a configuration that differs only in an
  agent's read-only `phaseGuidance` and records the difference, and
  `setup --lessons-from` keeps the stamped configuration copy
  `stamped-config-<sha16>.json` beside the lesson ledgers for that route.
- A closed Goal's head reads `CLOSED (revision N)`, and the Goal text never
  reports the closing route met while registered requirements are unmet.
- After the repair streak is exhausted, the Lead's recorded repair admits a
  worker return instead of a re-plan.
- `setup --task-card` snapshots a card outside the root into
  `.exitbind/plan-cards/`, every path input meets its root in one place, and
  one writer owns product files under a root.
- The legacy compatibility reader inventory lists every historical reader,
  and the never-persisted `SOULMATE_DRIFT:` and `SOULMATE_JSON:` in-process
  prefixes are retired.
- A gate inventory lists every native CI gate with its basis, and
  `ci-failure.sh --record` appends to a failure ledger; both are checked in
  the existing product-surface stage.

## 0.31.20

Published stable release; the `v0.31.20` tag and release assets are available.

- After a stable release is published, the release workflow dispatches the
  README badge refresh, which a release published with `GITHUB_TOKEN` never
  started on its own.
- Install through `releases/latest/download/install.sh`: each release
  publishes its own `install.sh` and `install.sh.sha256` as assets, the
  documentation-version inventory keeps every version mention current, and a
  release is published only after every asset is uploaded and read back.
- The prose-mechanism audit records preservation checks as mechanism and
  separates the `work validate` result from the choice to reuse a packet.
- Claude Code shows the Goal in its status line: `exitbind statusline` reads a
  per-session Goal display state written after each Goal or Work ledger
  command and at SessionStart, and `setup --apply` installs it unless a status
  line already exists. An approved Claude Code plan with a `Done when` table
  is saved as a private task card for `setup --apply --task-card`.
- `VALUE_PROOF_BASE` accepts a ref, resolved to its merge-base with `HEAD`.
- A closed Goal reads closed: the SessionStart hook prints one line saying the
  Goal is closed with nothing to resume, and the Goal text states the journey's
  member count and that it closed with the Goal, without a closing or read
  route. The JSON diagnostics are unchanged.

## 0.31.19

Published stable release; the `v0.31.19` tag and release assets are available.

- `setup --task-card PATH` registers the card's `Done when` rows as named
  requirements with `--apply` and prints the whole start path: `goal cover`,
  `goal prepare` and the named `work begin`, in that order.
- `goal prepare` and the named `work begin` derive the checker from the
  declared check command, directly or through one outside script it runs;
  `--artifact` becomes optional and the emitted begin route carries it.
- A plain-route Work begun under the owner's native Goal is shown as a
  journey member. After a host resume with no saved hint, the SessionStart
  hook and `session recovery show` name the open Works from the journey with
  their exact `work detail` command, open assignments and checkout trees with
  uncommitted changes. Claude Code's own session ID wins over a stale export.
- `setup --lessons-from PREVIOUS_CONFIG` carries the previous project's
  accepted, unexpired project lessons into a new project through the Lead's
  lesson lifecycle, and lists expired, revoked and changed ones with reasons.
- The Goal text in `goal status`, Work responses, `setup --apply` and the
  SessionStart hook states the journey (member count, closing route, read
  route for members not yet current) and the project-lesson state.

## 0.31.18

Published stable release; the `v0.31.18` tag and release assets are available.

- Deliver role-scoped guidance to the recipient that acts on it, through work
  detail, native start and prepared child context, within one byte budget.
- Keep opt-in guidance continuity advisory: launch-only guidance changes do
  not invalidate checks, while declared guidance verdict inputs and checker
  dependencies stay bound to the tested inputs.
- Add an optional adviser floor that native actions enforce when a
  configuration declares one.
- Show a bounded current milestone in native session recovery, derived from
  the native Goal and freshly rendered Work detail; a saved hint never selects
  the action.
- Give every Cargo test target its required features and covered `src`
  prefixes; pre-return selects the targets affected by a change and runs each
  with its features and without host session variables.
- Deliver owner-declared phase guidance bodies whole with a visible count, or
  by an exact role-scoped read route, and name the delegated agent in Lead
  detail as a sign rather than a refusal.
- Validate a successor's lineage once per journey lookup and reuse validated
  ledger bytes within one process, so successor commands under a native
  session Goal answer at once. Acceptance still rehashes current inputs.
- Print `work detail --view action` as plain text without `--json`, carry
  UTF-8 section pages and evidence as text, and add a `producer` envelope to
  observation JSON.
- Remove host session variables from every test that spawns the binary.

## 0.31.17

Published stable release; the `v0.31.17` tag and release assets are available.

- Add an opt-in project continuation path with source-grounded admission,
  visible status, stop handling and replay-safe Work accounting.
- Transfer exact named-Goal contributor mappings to a verified successor only
  after checking every selected reference; preserve unrelated mappings and
  recover matching partial commits without reviving retired Works.
- Refuse fresh associated retirement after a stop, source change or expiry
  before writing the retirement claim. Existing saved successors remain
  recoverable on a supporting reader.
- Keep historical strict readers fail-closed on marked Works. Retain the old
  pre-registration configuration for ordinary rollback and use a supporting
  executable for the marked Work lifecycle; stop does not rewrite old ledgers.

## 0.31.16

Published stable release; the `v0.31.16` tag and release assets are available.

- Carry approved optional pre-return diagnostics through ordinary setup and
  share owned verification context across focused, diagnostic and full checks.
- Preserve signals received while creating an owned check subprocess and retain
  its complete output and exit status.
- Apply qualified specialist experience consistently to actual native delivery;
  retain complete ordinary guidance and reject reserved-source recall bypasses.
- Own Linux observation descendants separately from unrelated caller children,
  and keep historical unknown-termination recovery inspection-only.
- Preserve complete recipient expansion when long invocation paths select the
  existing bounded action fallback.

## 0.31.15

Published stable release; the `v0.31.15` tag and release assets are available.

- Add optional early evidence and exact pre-return diagnostics while keeping
  final assurance, independent review and Lead acceptance separate.
- Deliver the existing whole-session completion sentence inside a simple ASCII
  box when the complete current Goal actually closes.
- Add explicit read-only specialist routing with scoped lifecycle, source and
  profile currentness checks; the host retains launch and permission authority.
- Split large implementation files by responsibility without changing existing
  supported paths. Existing Works retain their original configuration and producer.

## 0.31.14

Published stable release; the `v0.31.14` tag and release assets are available.

- Allow a successor Work's scoped submission to pass checked preflight against
  its exact committed prefix and one validated submit preview, while refusing
  changed history, malformed submit linkage, and additional proposed events.

## 0.31.13

Published stable release; the `v0.31.13` tag and release assets are available.

- Continue a defective frozen check through an explicitly approved linked Work,
  retaining the predecessor, same-Goal accounting and fresh check/review gates.
- Return current bound actions after mutations and distinguish genuinely absent
  optional Goal context from damaged or expected associations.
- Deliver whole-session completion as a compact event in the normal Lead response,
  with stable event identity and one read-only recovery route.
- Check installation pointers against exact available stable assets, and reuse
  native CI dependency builds while retaining platform and release checks.
- Include the inherited absent-basis discriminator and check-recovery wording fixes.

## 0.31.12

- Help the default Lead distinguish settled constraints, provisional ideas and
  consequential unknowns, and resolve costly uncertainty with one focused
  question or a small test while preserving accepted changes and side work.
- Provide an exact, explicitly approved recovery route for known-ended checks
  whose failure result could not be saved: it records the check as failed from
  the retained response without rerunning the check command or crediting a
  successful check.
- Refresh README release and CI status through isolated, ref-bound automation
  while preserving the existing CI trigger contract and packaged files.

## 0.31.11

- Inspect and recover retained native action results through a bounded, read-only route
  without replaying the completed provider call; refuse recovery when process or effect
  evidence is uncertain.
- Attach optional context from an identified provider-host conversation Goal to ordinary
  main and side Works, while keeping cross-project Goal evidence separate and completion
  output concise.
- Preserve exact current context and evidence routes when an action view exceeds the
  response budget, so the complete material remains reachable through current detail.

## 0.31.10

- Deliver reviewed project skills and lenses to applicable configured recipients
  on their next compatible native launch, with bounded content and current hashes.
- Hide inapplicable contradiction choices; expose a bounded current action view
  after permit/edit and retain complete instruction and evidence section routes.
- Explain project lesson source/fact bounds and stable correction identity before
  proposal; clarify singleton continuing-work obligations.
- Adopt the deliberate 0.31.10..99 release series, with immutable published
  artifacts and required attestation, installer and platform gates.

## 0.31.0

- Move the canonical repository, installer, update feed, schemas and plugin
  links to `PANIC-ANGEL-SCIENCE/exitbind`.
- Start the organization repository with one imported legacy baseline and
  retain subsequent Exitbind development commits individually. Retained
  author, committer and tagger dates are unchanged; rewritten object IDs and
  signatures differ. See [public history](docs/public-history.md).
- Existing installations need a one-time installation from the new repository
  to switch their update origin. Preserve pinned readers for active Work and
  historical records. Current commands, hook tokens and record formats remain
  compatible; Soulmate remains retired.
- Publish newly built artifacts under a new version and source provenance;
  earlier signed archives are not relabeled as builds of rewritten commits.

## 0.30.1

- Keep complete recovery responses within the existing 8 KiB output budget
  when short executable paths allow a longer configuration path to be inlined.
  Preserve exact configuration/executable recovery obligations and terminal
  state instead of duplicating a route beyond the complete response budget.
- Cover intermediate configuration lengths and validate stale-return recovery
  through canonical full detail when long fixture paths require compact
  navigation. Retain refusal, current binding, attempt and no-mutation checks.
- Preserve the original resume status and small unreadable-candidate details
  in emergency responses while retaining bounded counts for oversized lists.
- Preserve completed-goal details and all section references by sharing the
  selected executable and configuration through explicit recovery flags when
  repeated detail and receipt commands would exceed the response budget.

The immutable 0.30.0 tag is retained. Its release checks failed before assets
were published; this patch carries its accepted product changes forward.

## 0.30.0

- Retire the source-built Soulmate executable, its selectable runtime profile,
  and its updater route. Current Exitbind Run and Work behavior, historical
  readers, hook tokens, and existing project data remain supported.
- Keep the full default test suite, current native handoff, updater and
  installer checks while retiring duplicate Soulmate-target compilation.
- Add a stable request ID to ordinary worker permit forms so repeating the
  exact current invocation returns its recorded result without another spend.
- Add a narrow owner-attested, same-goal successor route for an exact adjacent
  duplicate-permit terminal block. It preserves the predecessor's accounting,
  requires verified no-effect evidence and fresh successor evidence, and never
  transfers grants, checks, review or acceptance.
- Keep pending worker stages available after a successful observed check, and
  bind worker re-plans to the current configured worker name and assignment.

- Include the accepted adoption, named-goal/evidence and CI recovery changes.
- Add explicit exact-item owner correction for a mistaken project-lesson recall
  policy, with preserved lineage, expiry and idempotent lost-reply recovery.
- Explain effective recall scopes and deliver configurable Lead display names,
  purpose and canonical recipient identities through ordinary project context.
- Ship outcome-first reporting and source-free recovery guidance, exact command
  output contracts and bounded complete-context consumption.
- Recover a cut first response through current compact navigation and 4 KiB
  exact section pages, including the current recipient's complete instructions.
- Refuse held or foreign build targets during preflight and retain boot/process
  identity with owned native check locks. Preserve current native platform,
  test, package, installer and handoff checks while retiring obsolete execution.

- Add configured-Lead recovery for an exact known-ended check storage failure.
  Bind the saved response and partial bytes to its original admission, record
  failure without another execution or accounting reset, then resume ordinary
  repair, fresh checks and required review. Uncertain outcomes remain blocked.

Policy correction writes memory v3, which older readers refuse. Preserve old
Work pins and retain a compatible memory reader during selector rollback.

## 0.29.0

Published stable release; the `v0.29.0` tag and release assets are available.

- Add a marked v8 `carryProtocol` v1 accounting seed to same-goal
  `run supersede` successors. Preserve phase, spent count, no-information
  streak, post-re-plan count, re-plan count, budget and defaults, seen-evidence
  fingerprints, and observation keys. The successor gets a fresh run identity,
  assignment and authority. Mutation pointers, grants and check evidence do
  not carry; the successor must check its current result. Older readers refuse
  the marked record without mutation; markerless historical records keep their
  existing meaning.
- Complete the ordinary handoff path with current role context, role-bound
  operations, and same-goal continuation while preserving prior Work records
  and decisions.
- Align active schemas, examples, installer and bundled plugin metadata with
  this release line. Add explicit upgrade and selector-only rollback guidance
  for custom files, pinned Work readers and newer marked state.
- Replace the README's specialist-first tour with a short path through the
  existing conversation, a first checked result, interrupted work and the
  lead's completion decision. No new product capability is implied.

## 0.28.0

Published stable release; the `v0.28.0` tag and release assets are available.

- Complete legitimate repeated check repair with a current carried re-plan over
  the permitted edit's inputs. Require frozen configuration and contract,
  preserve spent authority, and keep exhausted or uncertain work stopped.
- Deliver executable worker recovery and retained-result return forms in current
  Work detail. Fresh checks, required review and Lead acceptance still follow.
- Include external control/state topology diagnostics, the narrow supported
  controlled file-effect path, and producer-bound binary consumption accumulated
  since the previous stable release.
- Align current agent guidance with direct Work detail and provide optional
  requested Work closeout through the existing verified receipt owner. Replays
  verify the same receipt without repeating acceptance or checks.


- Integrate optional Architecture Contract history and add reviewed-source
  selection preview/apply through the existing project owner, with exact pins,
  drift refusal, returned action and custom configuration preservation.

- Retain bounded canonical check-observation admission/failure facts and route
  safe same-Work repair through the Lead, fresh worker result, checks and review.
  Unresolved effects block retry and supersession. Add ordinary Work deadlines
  and exact committed-check replay without executing the checker twice.

## 0.27.2

- Exercise stale-return protection on long native paths and read complete
  assignment detail when the bounded response omits its optional inline packet.
  Preserve the stale refusal, ledger and artifact nonmutation checks.
- Verify bounded Work recovery on long paths through complete metadata and
  exact history expansion, retaining assignment and subject agreement.

## 0.27.1

- Preserve current-Work selection, history, focus and discovery metadata through
  compact response fallbacks, including long native temporary paths.
- Complete the R24–R27 stable distribution after the immutable v0.27.0 tag
  failed native release checks before publishing assets.

## 0.27.0

- Assemble approved asymmetric role facts with model-free setup and keep
  accounted usage and current scoped context attached to the supported Work.
- Deliver reviewed, bounded project lessons in applicable assignments. Facts
  carry no previous Work checks, review approval, acceptance or task grants.
- Add explicit Lead-reviewed configuration revalidation for project lessons.
  Only additive recipient read rights and schema URL metadata are compatible;
  immutable old events remain readable. Revalidation writes memory-event v2;
  older binaries refuse that ledger and must not mutate it during rollback.
- Hash eligible lesson guards with a fixed streaming buffer, preserving
  no-follow checks, missing-file exclusion and observed change/refusal behavior.
- Expose one additive `effectiveAction` projection for programmatic Work
  consumers, with current bindings, exact detail routes and action forms.
- Validate workflow syntax and context availability before native builds with
  checksum-pinned, task-local actionlint. Acquisition is explicit; a provisioned
  archive supports offline checks. Reject the unsupported job `runner.temp`
  expression before hosted checks.

## 0.26.0

- Give the current assigned outcome and related goal obligations a readable,
  verified evidence path. Preserve exact originals, recipient scope and
  currentness while retaining expert and historical readers.
- Provide state-specific action forms for failed checks and pending review
  findings. Mechanical identities come from current state; repair, deferral,
  rejection and their reasons remain explicit Lead decisions.
- Align ordinary and native context delivery, default role responsibilities,
  packaged guidance and help. Explain the whole workflow in the README, from
  applicable goals and context through execution, return and trusted completion.
- Deliver current native project/role context and retry-safe managed worker
  reads and edits. Refuse private-evidence aliases and unsafe reconstruction
  after managed state loss; preserve recorded result replay and native authority.
- Show canonical goal and task status separately from current-result readiness.
  Emit the supported English status outside provider prompts and retain scoped
  bounded detail, current receipt routing and host child help.
- Include the optional static event-derived work view. Its motifs carry no
  authority and do not imply image exploration or autonomous memory ingestion.

## 0.25.2

- Recover a saved native result through the same Work, verify its recorded
  assignment and process identity, and replay an already recorded return without
  starting another provider process.
- Refuse automatic resume when execution is uncertain, an earlier bounded retry
  was spent, or recorded activity could have external effects. Preserve the
  failed operation and expose its current inspection and recovery route.
- Carry reviewer repair decisions into the next assignment while keeping
  current checks, reviewer evidence, and Lead acceptance separate. Existing
  run-event v8 records retain their identity and remain inspectable.
- Deliver composite-task closure and provisional causal-investigation guidance
  through the default Lead and Worker profiles and the managed skill. Preserve
  customized profiles, native authority, and the host's model choice.

## 0.25.1

- Run a bounded native Codex worker or reviewer from the current Work assignment
  with `work act`, structured command and usage observations, packet-bound
  reviewer evidence, explicit checks, and Lead decisions. Preserve failed turns
  in private journals and allow a worker to resume the same native session.
- Record a small direct Codex activity with `activity codex`, including command
  exits and a bounded before-and-after changed-file reference. Label its task
  outcome unjudged rather than treating the activity as governed acceptance.
- Preserve historical reviewer rework transitions when replaying older ledgers.
  Keep fallback reviewer journals separate and bound native timeout cleanup to
  the process group and output readers.
- Split oversized run, value, and CLI modules by responsibility while keeping
  their existing behavior and compatibility paths.
- Deliver reviewer evidence with executable, configuration, and working
  directory context; include requirement-bound check records and logs while
  keeping required profile and perspective context inside the host budget.
- Refuse to initialize named continuation requirements unless the Work's
  frozen checked preservation policy supports their exact IDs and text, and
  report compatibility without changing historical evidence. Explain that
  the current CLI supports one preservation requirement per Work and surface
  the design boundary when a continuation names several.
- Move carried-mutation evidence identity validation into its owned run module
  without changing event or error semantics.

## 0.25.0

- Deliver bounded install-owned project and host facts with current rules and
  exact read routes, plus explicit named-agent and task perspective acquisition.
- Distinguish a named whole goal's current readiness from its historical
  closure. The optional local terminal room appears only at current completion.
- Keep current Exitbind work mutations bounded with exact event recovery and
  product-derived next actions.
- End active Soulmate binary and skill distribution. Default builds and
  installs ship Exitbind alone; historical state readers and a feature-gated
  legacy CLI test fixture remain.

## 0.25.0-rc.5

- Keep an Exitbind executable's product identity when it is copied or renamed;
  preserve the legacy `soulmate` command and separately built Soulmate executable.
- Tell the Lead to repair clear in-scope defects without asking the owner again,
  while retaining the required disposition record.
- Route a reviewer `rework` in marked governed work to a Lead decision:
  `work disposition --decision repair|defer|reject|supersede` binds the
  pending finding cycle and basis. `defer` and `reject` start no work and
  resolve review as `resolved_by_lead_disposition`, not approval; `repair` and
  `supersede` carry the Lead's repair boundary to the next attempt. Repeated
  findings show prior cycles to the Lead. Default role profiles now separate
  findings from requirements.
- Export the host-reported Claude Code session ID to later shell commands as
  `EXITBIND_NATIVE_SESSION_ID` from the managed SessionStart hook.
- Start an explicit-locator handoff with `work continuation WORK`, whose
  `receive` block gives the bind and native-child commands; the managed
  SessionStart context names that route.
- Capture a prepared native child from the host: `work child prepare` records
  the intent, and the managed Claude Code SubagentStart and SubagentStop hooks
  record the host-reported child ID and exact final message. `--replace`
  supersedes a waiting intent, and `work child recover` records a retained
  result that failed to attach.
- Project the Lead's configured role and current accepted project memory into
  a new session's SessionStart context when memory is enabled; revoked,
  rejected, expired, changed, and other-project items are never projected.
- Keep a navigation-only current-work focus: `work begin` selects the new work,
  `work resume` returns it, older running work stays history
  (`work resume --history`), and `work focus WORK` repoints it.
- Add `exitbind version --json` with the embedded build commit and the running
  executable's SHA-256 for comparison with release evidence.
- Report missing memory scope arrays as configuration errors instead of
  panicking.
- Explain when to use Exitbind from its own behavior in the README.

## 0.25.0-rc.4

- Add supported receiving-host bind and native-child result commands that build
  canonical continuation records and exact content digests from a context token.
- Keep stale-context and conflicting-result refusals while making the normal
  handoff path independent of manual JSON, hash and revision bookkeeping.
- Clarify the current same-work capability, environment-preserving setup and
  evidence limits in installed guidance and public documentation.

## 0.25.0-rc.3

- Attach continuation to an existing open goal for the same work without
  replacing its history; identical retries leave that history unchanged.
- Keep continuation mutation replies bounded and emit exact read-only recovery
  routes with their executable and configuration context.
- Tie observed check support to the configuration recorded when the check ran.
- Guide a receiving Codex or Claude host through explicit work recovery and
  host-reported native child recording. Small reversible work stays direct.

## 0.25.0-rc.2

- Return bounded, exact-result responses from W1 work mutations and recovery
  commands, including explicit refusals and next actions.
- Preserve evidence for refused deliveries and verify recovery event identity,
  route, and artifact integrity before resuming work.

## 0.25.0-rc.1

- Let active human-directed runs continue with their recorded plan after
  configuration, profile, memory, boundary-content, or semantic harness drift;
  report the drift while keeping exact receipt and artifact integrity enforced.
- Raise observed check capture to 8 MiB per stream and 16 MiB combined, with
  bounded 64 KiB-per-stream expansion display.
- Separate observed process capture and cleanup from run admission and artifact
  ownership.

## 0.24.0-rc.7

- Align public author metadata and GitHub account coordinates with the
  maintainer's current public identity. The organization import updates these
  historical identity references; original source bytes remain private.
- Add a deterministic `.mailmap` regression check and contributor guidance to
  prevent future identity drift without rewriting published history.

## 0.24.0-rc.6

- Reduce needless progress/status polling in shipped guidance: reuse one native
  handle, wait for meaningful transitions, and keep unchanged state quiet.
- Align current CLI and legacy compatibility guidance with run-event v8 and
  historical v1-v7 readers.
- Bind checked-start subjects to the event run ID and reject reconstructed
  ledgers whose subject identity does not match the start event.

## 0.24.0-rc.2

- Make `work check` guidance explicit that the frozen command is executed and
  recorded once, and distinguish worker/reviewer blocked attempts from an
  owner decision boundary.
- Preserve explicit `--basis` and `--review-policy` values when creating a
  successor with `run supersede`, and bind the successor subject to its basis.
- Add deterministic coverage for successor basis and owner review-policy
  propagation while retaining historical no-option behavior.

## 0.23.1-rc.2

- Keep ordinary low-consequence work outside Exitbind project initialization
  when the target has no applicable governed policy.
- Distinguish clean unconfigured absence from malformed, unsafe, or unresolved
  target configuration; unresolved routing never claims native-only status.
- Preserve existing governed project behavior and host-managed skill paths.

## 0.23.1-rc.1

- Make direct handling explicit in installed guidance and session hooks: routine
  low-consequence edits need no setup, workflow, or review-policy questionnaire.
  File names alone do not select governance; existing scoped decisions carry on.

- Let owners initialize configuration and private state with `init --skip-skills`
  when another tool manages project skill paths. Existing files and symlink
  targets remain untouched; default initialization retains its protections.
- Report skipped guidance installation explicitly and reject conflicting setup
  options before mutation. Existing projects and run formats are unchanged.
- Publish release candidates as prereleases without replacing the stable latest
  release, with channel-matching checks for existing releases.

## 0.23.0

- Reorganized responsibility boundaries across the governor/kernel, CLI,
  configuration, run/work, host, project, memory, compatibility, and
  distribution modules; public command and historical-record compatibility is
  preserved across the move.
- Added a conditional Lead-owned basis with causal successor records, while
  keeping implementation details and responsibility boundaries explicit.
- Added an owner-selected review policy that can be revised on a marked running
  run; omission remains distinct from review approval.
- Added consequence and promotion classification so direct, governed, and
  unavailable activation outcomes are reported from typed facts.
- Added current session-goal integrity and direct completion evidence, with
  presentation and cache behavior bound to the same validated state.
- Updated host guidance, project/plugin projections, release coordinates, and
  platform checks for the 0.23.0 surface.

## 0.22.0

- Added deterministic role-specific thin context projections with exact
  expansion references and replay-safe stale/tamper refusal.
- Added a provider-neutral cooperative checkpoint governor with a finite hard
  iteration budget and conservative, at-most-once sensor input handling.
- Added the canonical `work permit` action: it revalidates the current
  assignment and tested inputs and consumes a governor unit in the run ledger
  before a cooperative product mutation; completion does not double-count it.
- Extended the context-surface inventory with reproducible HEAD/current JSON
  measurements; historical run-event formats remain readable.

## 0.21.0

- The terminal block is offered on the low-level run surface too. A lead that
  drilled down to `run status` found no product wording there and composed its
  own closing sentence; both surfaces now read the same rule, so the run surface
  ends on the same block and `run status --json` carries it as `terminal`. The
  guard is unchanged: a changed tree ends the block.
- The host bridge can no longer be downgraded by an older process. `exitbind
  update` hands post-update bridge synchronization to the newly installed
  binary, and an up-to-date binary reports what is missing instead of
  reinstalling it. Bridge writes go through the hardened settings writer:
  a path escaping the home root, a directory at the target, or a changed file
  is refused, and file permissions are preserved.
- Legacy hook retirement matches only the two exact published record shapes, so
  a foreign hook that merely mentions the old command is never removed.
- The terminal status line is the product's to write, not the host's. A
  presentation block carries `terminal` - exactly `EXIT READY` when the
  canonical acceptance decision says so, and nothing otherwise - and every work
  response carries that block at the same path: `work next`, `work resume`
  (including when nothing is active and the last run is reported), and the
  decision that reaches the state. Both distributed guidance copies say to
  print the value on a line of its own with nothing attached to it.
- The session context a configured project injects names the terminal rule, so
  a lead that never opens the skill file still knows to print the value it is
  handed rather than compose a sentence around it.
- An acceptance stops speaking for the tree once the tree moves. A terminal
  run's identities are historical, so the terminal block is offered only while
  the exact tested inputs the acceptance was bound to are still the ones on
  disk; the recorded decision stays readable either way.
- `exitbind work resume` reports the most recently finished run when no work is
  active, instead of answering only `no_active_work` and leaving the reader to
  summarise the ledger itself.
- The presentation cache names itself in its own bytes as a derived display
  memo with no authority, and a file that does not is read as nothing and left
  untouched.
- `exitbind work next` and `exitbind work resume` return a `presentation`
  block: percent progress, the exit state, a classified state summary, and at
  most one transition phrase. Reading the same state again says nothing new.
- Progress now divides the check weight by the number of required checks per
  worker, and a review that no longer fits the tested inputs earns no review
  progress.
- `exitbind host install --all` is accepted again; the flag was documented but
  rejected by the argument parser.
- Updating from `v0.20.0` or earlier needs one repair afterwards. That released
  updater re-synchronises the host bridge from its own embedded copy once the
  installer has finished, so the older guidance is left in place; run
  `exitbind host install` once with the new binary, or install directly with the
  installer command. Code already inside a published release cannot be changed,
  so this is a recovery step, not a fix. From this release on, the newly
  installed binary owns that step.
- The presentation cache is written through the hardened managed-file writer and
  read without following symlinks: a file planted at the cache path is refused
  rather than overwritten, and a cache that cannot be read is left untouched and
  treated as absent. Losing the cache can only repeat an optional line; it never
  changes evidence, acceptance, or reuse.
- `exitbind init --refresh-skills` refreshes an Exitbind-managed project skill
  again. It recognised only the previous product's marker, so the command the
  CLI tells operators to run refused its own files.
- Presentation classification is derived from the same canonical run facts the
  work decisions use, so rewording or translating human help can no longer move
  a state or a transition. A partial set of passing checks is no longer reported
  as current.
- Preservation no longer depends on a second installation. The distribution
  carries `skills/exitbind/references/preservation.md`, projects it beside the
  project skill for both hosts, and ships it in the plugin bundle. A governed
  run with accepted requirements resolves its own route and quality - every
  formal task is assigned `FULL` - and returns that assignment in
  `humanHelp.preservationAssignment`. The preservation line now reads
  `Holytail :FULL · FORMAL · evidence=<how the evidence was acquired>`, where
  the evidence axis distinguishes a check Exitbind ran itself from one a host
  reported, and never claims independent verification.
- Temporary files written during normal use are Exitbind-named. The rename
  history moved out of the README into
  [docs/legacy-compatibility.md](docs/legacy-compatibility.md), and a
  regression test proves a fresh install and project show no previous-product
  identity while a historical ledger stays readable under its own.

## 0.20.0

- Installing Exitbind now installs the host bridge the coding-agent lead needs
  to find it. `install.sh` installs or refreshes a managed bootstrap skill for
  each detected host (`~/.codex/skills/exitbind/SKILL.md`,
  `~/.claude/skills/exitbind/SKILL.md`) and the managed session hook, then
  reports what it installed or could not install. A binary installed without a
  usable host bridge is no longer a silent state.
- Add `exitbind host status` and `exitbind host install`. Status reports the
  binary, the bootstrap skill, and the activation hook per host as separate
  observations, and states that discovery is not activation.
- `exitbind update` refreshes a stale managed bootstrap so a current binary
  cannot keep an older host bridge, and reports each refresh.
- Installation retires a superseded `soulmate hook-run` session-hook record on
  the Exitbind surface and keeps every other hook. Files Exitbind does not
  manage - a foreign skill at the same path, a symlink, another tool's hook -
  are reported and never overwritten.
- A session in a repository that is not configured yet now receives a small
  bootstrap context on the Exitbind surface: select Exitbind before
  consequential mutation for material work, keep tiny and read-only work
  direct, request the single initialization write, and do not claim activation
  without a lifecycle action. The legacy Soulmate surface stays silent.
- The unconfigured-project error names the single owner-approved write and its
  exact command instead of a bare instruction.
- Selection remains a host/model judgement. Discovery, selection, activation,
  and governed work are separate observations, and no prompt text forces
  compliance. No persisted format changed.

## 0.19.1

- Fix `exitbind update` on Linux leaving a zero-byte binary after reporting
  success. After the installer replaced the running binary, the updater
  re-derived its caller surface from an executable name that read as deleted,
  switched to the legacy profile, and copied the new binary onto itself. The
  surface is now decided once per process, and an install path is never copied
  onto itself.
- The installer moves a replaced binary aside into one hidden
  `.exitbind-previous-PID/` directory that keeps its file name, so released
  0.18.0 and 0.19.0 updaters no longer truncate the binary this installer
  places. The next install removes earlier aside copies.
- If an earlier update left a zero-byte `exitbind`, reinstall once with the
  pinned installer command in the README.
- No persisted format or public command changes.

## 0.19.0

- Write run-event format 6 for new Exitbind starts. v6 checks, reviewer
  approvals, protections, and lead acceptance record the tested-input identity
  of the product root (`product-root-files-v1`: tracked plus untracked,
  non-ignored files in a Git worktree, or every file otherwise; `.git` and state
  directories excluded). Evidence taken on different inputs is not current, a
  check whose inputs change while it runs records nothing, and acceptance
  recomputes the identity. Ignored files, files outside the product root, the
  environment, and remote or time-dependent conditions are not covered.
- 0.18.0 binaries refuse v6 ledgers; v1-v5 ledgers remain readable under their
  original guarantees and gain no input binding.
- Preservation requirements are v6-only and require a functional check policy;
  the combination without `checkPolicy` is refused at start, successor, and
  replay.
- Residual packets are version 2: the snapshot carries tested-input identity,
  and `humanHelp` is derived from state with `ownerDecision` separate from lead
  actions. Add `work validate WORK --packet FILE`, returning `usable`,
  `refresh_required`, `cannot_establish_applicability`, or `not_continuable`.
  Validation compares every behavior-relevant packet field and always returns
  the canonical packet to act on; only `usable` permits skipping work, and
  nothing in a submitted packet is executed.
- Derive `work next`, `work resume`, `work check`, and `work validate` from one
  run snapshot per request, so the next action, evidence assessment, packet, and
  help describe one ledger revision and one established input context. A failed
  input listing makes dependent evidence unavailable instead of falling back to a
  stored digest.
- Treat terminal runs as history: an accepted, rejected, or blocked run keeps
  its replayable record, but its packet carries no reuse and cannot authorize
  skipping new work after files change.
- No token, model-call, or human-time savings are claimed.

## 0.18.0

- Add deterministic residual packets to `work next` and `work resume`, reporting
  current work identity, subject identity, established facts, still-valid
  evidence, remaining obligations, the next action, and actions the next agent
  should not repeat.
- Let resumed work carry forward a current passed check or approved review when
  the exact subject remains valid, while refusing to reuse stale evidence after
  the subject changes.
- Document the bounded cost-evaluation fixture set for duplicate check/review
  avoidance and stale-subject refusal. No token, model-call, human-time,
  host-startup, or frontier-agent correctness savings are claimed.
- Preserve persisted run-event format 5, historical v1-v4 readers, and the
  existing local-only architecture; no daemon, cloud, database, telemetry, model
  runtime, provider integration, or dependency is introduced.

## 0.17.0

- Rebrand the current public product as Exitbind with the lines “Exitbind — No
  result exits unbound.” and “Bind the evidence to the exact result before
  exit.”
- Add run-event format 5 for new writes, binding an Accepted Subject to the
  exact result and evidence while retaining historical v1–v4 readers and
  `soulmate` producer provenance.
- Retire Coffee as a public feature and brand; readiness guidance belongs to
  the lead surface. Keep OpenCode path-compatible and experimental unless host
  execution is directly observed.
- Preserve the honest limits: local evidence is not tamper-proof, model
  compliance, human adoption, or general project-diff coverage, and no daemon,
  cloud, database, telemetry, model runtime, or new dependency is introduced.

## 0.16.0-rc.1

- Add the agent-first `work begin|next|return|check|resume` façade over the
  existing strict run core. It allocates routine ledgers and artifacts, returns
  opaque handles plus one next action, and keeps low-level `run` commands as the
  advanced interoperability surface.
- Establish a project-scoped selective-use contract: later supported agent
  sessions classify material work, activate Soulmate before scoped mutation,
  keep tiny reversible work direct, and return an explicit blocker when
  selected governed handling cannot activate.
- Separate available, discovered, selected, and activated states in generated
  skill and setup guidance. Fresh initialization now refuses stale managed
  projections instead of claiming exact embedded bytes and points to the
  explicit skill-refresh path.
- Move the complete low-level, recovery, and evidence guidance into a delayed
  reference while keeping the always-loaded skill focused on selection,
  authority, and the normal high-level path.
- Make bridge assumptions explicit in Reality claims guidance: keep endpoint
  facts distinct, test the connecting edge, and bind evidence to its target.
- Preserve run-event formats and existing v1-v4 readers; no migration, daemon,
  model runtime, permission grant, or new dependency is introduced.

## 0.15.0-rc.3

- Bound local `run observe-check` execution with a configurable timeout, run it
  outside the ledger lock, terminate its POSIX process group on timeout, and
  revalidate the complete durable binding before append.
- Distinguish locally observed, host-reported, and mixed check acquisition in
  human status while keeping check, review, and lead acceptance separate.
- Route setup-purpose agents from contributor instructions to README onboarding
  and keep Coffee's context guidance role-based.

## 0.15.0-rc.2

- Separate human contribution guidance from repository instructions for coding
  agents and keep Coffee independent of private tool-stack vocabulary.
- Select the contributor and CI Rust toolchain once in `rust-toolchain.toml`
  while retaining the package MSRV as a distinct Cargo contract.

## 0.15.0-rc.1

- Add checked run-event format 4 and `run observe-check`, which records a
  locally observed frozen command with separate acquisition and exit/signal
  result evidence while retaining v3 caller-reported ledgers.
- Reject replayed check events after terminal run state and clarify that update
  and skill diagnostics do not establish host-cache or active-session reload.
- Lead the public README with conversation-first, URL-first setup while keeping
  selective governed work, prerelease consent, and direct simple work explicit.

## 0.14.0-rc.4

- Lead with the existing host conversation, selective project preferences, and direct ordinary work while keeping setup and protocol boundaries explicit.
- Add fresh StateRoot ledger-path guidance and document current observe-path and migration-mode limits with focused regression coverage.

## 0.14.0-rc.3

- Clarify empty starter boundaries, update rollback diagnostics, and align the benchmark and setup guidance with their observable behavior.

## 0.14.0-rc.2

- Add a disposable behavioral-evaluation rubric that records observable probe and mutation ordering without claiming agent behavior or product benefit.

## 0.14.0-rc.1

- Add target-bound readiness guidance before materially costly dependent work or cutover while safe independent work continues.
- Lead the installer to the setup-free benchmark before portable project initialization.
- Align plugin presentation with the public product statement and correct the portable-root manifest schema usage; plugin installation does not install the separate CLI.
- Add an active SessionStart agent advisory for newer public releases and an explicit `soulmate update` path; neither performs an automatic install.
- Use a disposable user cache with bounded public release metadata; no persisted-format, dependency, or migration change is claimed.

## 0.13.0-rc.1

- Add canonical Soulmate guidance for bounded reality claims and decision closure.

## 0.12.1-rc.3

- Ship the `v0.12.1-rc.3` prerelease from source snapshot `4199ebd`, with native builds, packaged installation, and public asset installation validated for Linux x86_64 and both Mac architectures; WSL exercises the same Linux artifact inside Ubuntu. See [platform support details](docs/platform-support.md) and [public release workflow](https://github.com/envy404/soulmate/actions/runs/34094355184).
- Show current worker claims, host-reported checks, reviewer findings, and actual lead decisions in the human status and explanation views. A protocol refusal remains distinct from lead rejection; machine JSON and persisted records retain their existing shape.
- Add read-only skill diagnostics and documentation navigation and guidance. No schema, authority, or stable-policy change is claimed; no persisted migration is required.
- Frame the preview as a conditional choice for teams that want an inspectable acceptance record after handoffs or rework. Existing native worker/reviewer spawn capability must be confirmed by the coordinating host before a run; a profile, task name, or plan-only brief is not proof, and an unavailable tool returns pending work without a substitute executor. This is host guidance, not binary enforcement.

## Public tags and format readers

For a checked run using run-event format 3, retain a **0.12.0 or later
compatible binary**. For format 4, retain **0.15.0-rc.1 or later**. Inspect with
`soulmate run inspect LEDGER --config CONFIG`; do not edit a ledger's format
number to make a downgrade work.

This frozen map covers selected public format-transition and retained-reader
tags observed through `v0.15.0-rc.3` at commit `4faf2a7`. Reader support below
is established from tagged source; current-reader tests also replay frozen
historical fixtures. It is not a claim that every older downloaded binary was
re-executed during this review.

| Version | Public tag / source commit | Persisted format change | Inspect with this binary |
| --- | --- | --- | --- |
| 0.11.0 | [v0.11.0](https://github.com/envy404/soulmate/tree/v0.11.0), `7f1d014` | No new persisted format | Receipts 1–2; run events 1–2; memory, configuration and harness manifest 1. Rejects run events 3. |
| 0.12.0 | [v0.12.0](https://github.com/envy404/soulmate/tree/v0.12.0), `349b662` | Adds checked run events 3 | Receipts 1–2; run events 1–3; memory, configuration and harness manifest 1. |
| 0.12.1-rc.1 | [v0.12.1-rc.1](https://github.com/envy404/soulmate/tree/v0.12.1-rc.1), `23e04df` | None | Same persisted readers as 0.12.0. |
| 0.12.1-rc.2 | [v0.12.1-rc.2](https://github.com/envy404/soulmate/tree/v0.12.1-rc.2), `243d781` | None | Same persisted readers as 0.12.0. |
| 0.15.0-rc.1 | [v0.15.0-rc.1](https://github.com/envy404/soulmate/tree/v0.15.0-rc.1), `8b3a509` | Adds checked run events 4 with locally observed or caller-reported check acquisition | Receipts 1–2; run events 1–4; memory, configuration and harness manifest 1. |
| 0.15.0-rc.3 | [v0.15.0-rc.3](https://github.com/envy404/soulmate/tree/v0.15.0-rc.3), `4faf2a7` | None | Same persisted readers as 0.15.0-rc.1. |
| 0.17.0 | [v0.17.0](https://github.com/envy404/exitbind/tree/v0.17.0), `55d2c97` | Adds new checked run-event format 5 with Accepted Subject bindings; retains historical readers for v1–v4. | Exitbind v5 plus historical v1–v4 readers. |
| 0.18.0 | [v0.18.0](https://github.com/envy404/exitbind/tree/v0.18.0), `fc0ef16` | None | Same persisted readers as 0.17.0. |
| 0.19.0 | [v0.19.0](https://github.com/envy404/exitbind/tree/v0.19.0), `02df86f` | Adds run-event format 6 with tested-input identity bindings; retains historical readers for v1–v5. | Exitbind v6 plus historical v1–v5 readers. |
| 0.19.1 | Current stable Exitbind release | None | Same persisted readers as 0.19.0. |

The parentless public root `a0b8be3` contains package 0.10.0 without a
corresponding public release tag in this observed map. Earlier changelog
entries describe development history, not publicly reconstructable release
objects. They attribute receipt 2 / harness manifest 1 to 0.4.0 and run events 2
to 0.7.0; the original archived trees are not asserted equivalent to public tags.
Private archives, refs, and operational evidence remain intentionally
unpublished. See [the public history boundary](docs/public-history.md).

## 0.12.1-rc.2

- Lead with the existing setup-free experiment: `soulmate benchmark` runs a disposable
  configuration-repair example with real local checks and scripted actors.
  Its readable result shows the worker claim, failed check, reviewer approval,
  refused lead acceptance, rework, and the fresh accepted attempt.
- Lead the README with the installed-binary experiment, then an explicit
  `soulmate init --mode portable` project setup and a handoff naming the
  generated skill and configuration for the existing coding-agent host.
  Setup does not activate agents or grant host permissions.
- Exercise the literal README journey in a Git-initialized fixture while
  preserving the user's project and the explicit Git initialization guard.
- Preserve commands, JSON, proof scenarios, and persisted formats. Stable
  0.12.0 remains available; this preview correction adds no product invariant,
  stable-policy change, or migration. Synthetic checks establish observed
  refusal and recovery, not human time savings or universal installation payback.

## 0.12.1-rc.1

- Preview the optional `run submit --event-id` and `run next --text` forms,
  checked-start guidance, read-only next actions, and executable rework example.
  Default JSON and persisted formats retain their existing contracts.
- Lead the README with an existing-host task and real check. Bundle guidance
  for concise results with inspectable evidence, and describe initialization
  as preparing skills rather than activating agents.
- Report the bounded native/Soulmate pilots, including a real bug first found
  by the native baseline and missed by an initially accepted Soulmate run.
  These results do not establish better code or lower operator cost.
- This is a prerelease of compatible presentation additions. The current stable
  version policy does not classify these additions; no new product invariant
  or stable-policy change is claimed. Stable 0.12.0 remains available, with its
  JSON workflow. Upgrade the binary before using the new flags, then explicitly
  refresh managed skills. No persisted migration is required.

## 0.12.0

- Add opt-in checked acceptance: freeze a check command at run start and bind
  caller-reported results to exact current worker submission events. Missing
  or failed results prevent canonical acceptance, including during replay.
  The host executes checks; reported success is not authenticated execution,
  reviewer approval, or final acceptance.
- Add status, explanation, and redacted local aggregate views, plus a
  token-free `soulmate benchmark` scenario that reproduces refusal and fresh
  rework recovery. Synthetic command counts and elapsed execution do not
  establish human time saved or recurring production incidents.
- Checked runs use run-event format 3. Ordinary v1/v2 runs keep their existing
  format and read behavior. Older binaries reject v3; retain a compatible
  binary to inspect checked evidence and never downgrade a ledger by editing
  its version. No mandatory runtime or dependency is added.
- Link the scoped public claim to its executable proof and pin versioned proof
  schemas in CI. The minor version follows the existing invariant rule: a
  configured failed-check counterexample previously accepted is now refused.

- Strengthen contributor release-reference checks and pin the retained public
  ancestry in CI. Document the existing history boundary and separate retention
  rules from publication permission. Product behavior and version policy are
  unchanged by those contributor-only checks.

## 0.11.0

- Made native conversation preservation an explicit host-guidance invariant:
  profiles, briefs, run records, and governed memory augment the active root
  conversation. They do not authorize replacing it or discarding recent user
  corrections, rejected approaches, or their rationale.
- Session-start hooks present the same bounded advisory distinction. Executable
  hook and generated-skill regression checks cover the behavior Soulmate owns.
- Added a diagnostic guide that separates thread replacement, compaction,
  injected context, and host/client integration hypotheses.
- This is synchronized guidance and tested local hook behavior, not proof of
  model compliance or resolution of a historical host-memory regression.
- Migration: update host guidance with the supported skill-refresh path. No
  CLI command, configuration, persisted format, dependency, or model runtime
  changes are introduced. The minor version records the new product invariant
  under the repository's versioning policy.

## 0.10.0

- Attended implementation workers and reviewers must now use the host's native
  subagent spawn with the assignment's exact native task name; when that spawn
  is unavailable, stop and return the pending assignment to the operator. This
  is synchronized host guidance, not Soulmate enforcement of host execution.
- `soulmate away` remains reserved for an explicit operator-away or disconnect
  continuation; the Rust away runtime is unchanged.
- Corrected hook-output home-path redaction so machine-local paths stay
  redacted while the public source remains safe for publication.
- Migration: update attended workflow handoffs to native spawn. This release
  adds no CLI, configuration, persisted schema or format, dependency, or
  runtime expansion.

## 0.9.3

- Removed the unsupported context-observation collector and report from the
  product tree and release binary. The implementation remains available for
  transparent development only on `experiment/context-observability`; locally
  collected diagnostic data remains private and is never published.
- Local-mode hooks now resolve the exact ControlRoot through the private
  machine binding while confirming that the hook ProductRoot still matches;
  repeating `soulmate bind` upgrades compatible older bindings in place.

## 0.9.2

- Added strict private ingestion of exact host-reported usage for internal
  context dogfooding, correlated to content-free run and assignment identities.
- Reports exact repeated and still-pending assignment invocations without
  treating an invocation as completion, success, or authority to skip work.
- Rejects malformed, duplicated, rerouted, unsafe, unmatched, and concurrently
  stale diagnostic evidence while leaving the default CLI and canonical
  persisted formats unchanged.

## 0.9.1

- Added non-default, private context-accounting dogfood instrumentation that
  measures only Soulmate-owned byte boundaries and keeps model/provider usage,
  host context, and broader optimization claims explicitly unknown.
- Hardened authority classification for mixed payloads and run-scoped
  observations without changing the default CLI or canonical persisted
  formats.

## 0.9.0

- Native away recovery now records the explicit or unknown sandbox posture and
  distinguishes exit codes from signal termination. Completion requires the
  exact stage, attempt, and agent submission event; merely leaving the pending
  set is reported separately.
- Path inputs used by commands, identifiers, or recorded evidence now reject
  non-UTF-8 rendering and unresolved working directories instead of silently
  substituting or collapsing paths. A repository allowlist gate prevents the
  rejected patterns from returning.
- Release and plugin manifests now share one version identity, and tag
  publication runs the release-reference gate before building artifacts.

## 0.8.0

- New projects and explicit `migrate paths` runs prepare the complete public
  `soulmate/` and private `.soulmate/` directory contract while retaining
  legacy manifest and evidence bytes.
- `doctor` now distinguishes a direct dotagents command from uninvoked
  `npx`/`agents.toml` observations.
- Clarified that external task handoffs are incomplete unless exact observe,
  write, and command authority arrives as structured fields rather than guide
  prose.
- Separated authoritative assignment contract, reviewed profile guidance,
  context-only memory, and evidence-only harness/artifact claims in the native
  away prompt, with instruction-like adversarial fixtures and security limits.
- Added an explicit deterministic `migrate layout` dry-run/apply path for
  configured `.agents/profiles/` sources. It refuses symlinks, collisions, and
  tracked or staged mutations while leaving historical evidence untouched.
- `run supersede` now permits one provenance-bound successor from a terminal
  `blocked` run while preserving immutable predecessor bytes and continuing to
  reject `accepted` and `rejected` predecessors.
- New portable and local projects, plus newly imported profiles, now keep
  canonical agent profiles under `soulmate/agents/`; existing configurations
  that reference `.agents/profiles/` remain valid and are never rewritten.
- Documented the Soulmate-centered directory contract while retaining
  `.agents/` for optional distribution and `.codex/`/`.claude/` for host-owned
  projections.

## 0.7.0

- Added opt-in run-event version 2, which binds an existing receipt-v2 harness
  record by StateRoot-relative path and exact SHA-256 while retaining generation
  and inspection of unbound version-1 runs.
- Added native `soulmate away` start/list/show commands. The single Rust binary
  now revalidates config, selected profile/runtime, boundary, memory, upstream
  artifacts, receipt, and exact manifest claims across an isolated tmux launch.
- Removed the project-copied Python away adapter and its Python CI dependency;
  0.6 scripts migrate to `soulmate away` without changing receipt/config shapes
  or the canonical `run submit` evidence path.
- Added fail-closed receipt/manifest drift, mixed-event-version, plan-coverage,
  zero-Codex-launch, recovery-state privacy, and real-tmux regression coverage.

## 0.6.0

- Added an optional, project-scoped Codex+tmux reference adapter that keeps one
  already-authorized pending assignment alive across an operator disconnect.
- Kept normal `run next`/`run submit` ledger evidence canonical; adapter status
  is private process-recovery evidence and no daemon, provider client, second
  JSONL store, persisted schema, or dependency was added.
- Revalidates the exact assignment, profile, memory hashes, runtime choice, and
  fresh StateRoot artifact path before launch; rejects fallback selection and a
  second same-host launch of the same live assignment.

## 0.5.0

- Made generated and published schema references follow the crate version while
  accepting older advisory harness `$schema` values for manifest version 1.
- Clarified that `independently_verified` records an off-box claim whose format
  is validated and bound, without hashing a local artifact or authenticating a
  verifier; SHA-256 inputs must be lowercase hexadecimal.
- Hardened onboarding path errors, documented fixed evidence shapes and the
  canonical-root precondition, and clarified macOS source-test-only support.

## 0.4.0

- Added an opt-in, versioned harness manifest that hash-binds portable project
  and session identifiers, harness identity, and bounded skill, perspective,
  and Ponytail activation evidence into the existing canonical receipt without
  copying raw manifest strings.
- Added receipt version 2 for harness-bound receipts while retaining version 1
  generation and verification when no manifest is supplied.
- Kept evidence levels explicit (`configured`, `presented`, `agent_declared`,
  `hook_observed`, and `independently_verified`) without treating presentation
  or declaration as proof of agent compliance.
- Rejects unknown manifest fields and free-form content; prompts, goals,
  transcripts, secrets, raw environment values, and unrelated project content
  are not accepted or copied into receipts.

## 0.3.0

- Removed the legacy Node implementation and moved publication privacy and
  compatibility coverage into Rust tests with frozen v0.0.8, v0.1.x, and
  v0.2.x ledger fixtures.
- Hardened Git worktree preflight against non-UTF-8 paths, missing Git,
  command failure, and inconsistent tracked/staged failure handling.
- Removed external-input panic paths from boundary, CLI JSON, timestamp, and
  memory path handling; `--json` failures now remain parseable.
- Continued the typed configuration transition for boundary, receipt, runtime,
  and memory selection without changing persisted receipt, ledger, or config
  shapes.
- Reorganized the bundled skill around recovery branches and distinguished
  configured, presented, agent-declared, hook-observed, and independently
  verified harness evidence in existing submitted artifacts.
- Pinned generated schema/install references to the release tag, added Linux
  dependency audit and macOS test coverage, and reduced duplicated README
  limitation prose in favor of the security boundary document.

## 0.2.1

- Fixed empty CLI invocation and added conventional `--help` and `--version`
  entrypoints without changing existing subcommand parsing.
- Added content-free memory-budget refusal diagnostics with the attempted
  item/byte totals at refusal and configured limits; strict no-truncation
  behavior is unchanged.
- Began a gradual typed configuration transition at the validated agent/runtime
  boundary while preserving raw configuration bytes and persisted formats.
- Added GitHub artifact provenance attestation and release-workflow verification
  for the packaged archive, while retaining installer SHA-256 checksum checks.
- Clarified that root Node files are preserved v0.0.8 compatibility/privacy
  fixtures, not a supported installed runtime.
- Fixed skill refresh to restore missing managed copies and report created,
  refreshed, or unchanged destinations truthfully after complete preflight.

## 0.2.0

- Added explicit portable and local repository modes with typed ControlRoot,
  ProductRoot, and StateRoot ownership, private machine-local project bindings,
  and Git preflight refusal for tracked or staged runtime state.
- Added exact run-scoped observe/write manifests that can only narrow configured
  maxima, remain hash-linked across resume, and fail closed on manifest drift.
- Added warning-level placeholder diagnostics and focused local-mode,
  publication-boundary, narrowing, drift, and parallel-assignment tests.
- Added immutable, attempt-scoped StateRoot artifact hints and explicit
  `--artifact-root state` submissions so rework does not overwrite prior
  evidence.
- Added producer version/commit evidence to newly created receipts and ledger
  events while preserving inspection of older records without that field.
- Updated Rust examples and CI, declared Rust 1.75 as the MSRV, and prevented
  release workflows from overwriting existing tag assets.

- Added opt-in, project-scoped recall of accepted memory evidence through a
  shallow bounded ledger root and exact per-agent scope/context filtering.
- Added content-free `memory resolve` output and deterministic memory references
  in briefs, plans, and native-host run assignments; source bytes remain in
  operator-owned project files, while the bundled skill requires exact reads
  immediately before a matching subagent spawn.
- Frozen run references now fail closed on source, lifecycle, ledger-head, or
  selection drift without breaking inspection of existing v0.0.8/v0.1.0
  ledgers.
- Supersession now rolls back only its own newly created predecessor claim when
  successor creation fails, so a corrected retry is not stranded.
- Kept semantic search and embeddings out of core; any future optional ranker
  may operate only after lifecycle and authorization eligibility.

## 0.1.0

- Replaced the user-facing Node.js runtime with a Rust 2021 single binary.
- Added Cargo build metadata, embedded skills, release artifacts, checksums,
  and an initially Linux x86_64 installer.
- Preserved the v0.0.8 JSON configuration, receipt, JSONL run, and memory
  evidence formats; Node files remain only as migration reference fixtures.
- Kept configuration drift fail closed and added explicit `run supersede`
  provenance: a new bounded ledger links to a verified predecessor head while
  leaving the predecessor unchanged and sealed against later submissions.
- Clarified that role outcomes are evidence records, not votes or proof of
  multi-agent consensus; the configured lead retains final acceptance authority.
- Initially support Linux x86_64 release artifacts; unsupported platforms fail
  before download rather than receiving an unverified cross-compiled binary.

## 0.0.8

- Added explicit `soulmate init --with-coffee` and matching refresh support to
  install the bundled Coffee skill for Codex and Claude without changing the
  default Soulmate-only initialization path.
- Added all-selected-destination preflight and regression coverage so Coffee
  opt-in refuses conflicts before writing any selected skill file.
- Documented a narrow Venus interoperability boundary: cross-session
  orientation stays advisory and human-narrowed, while Soulmate owns only the
  active task's bounded handoff and run evidence.

## 0.0.7

- Repair the public CI installed-package smoke test by creating its isolated
  project root before initialization.
- Route CI privacy checks through Soulmate's tested, fail-closed privacy gate
  so adversarial test fixtures are not mistaken for release leaks.

## 0.0.6

- Added resumable `run start|next|submit|inspect` lifecycle evidence with
  role-aware transitions, rework attempts, artifact hashes, drift checks,
  bounded assignment batches, and conservative project-local lock recovery.
- Added bundled native-host adapter guidance: capable hosts perform native
  spawning and review while the CLI remains provider-free and records only
  declared evidence and submitted artifacts.
- Added private `.soulmate/` run state with an ignored contents boundary and a
  repository privacy gate; the gate is a publication check, not proof of
  purging local or untracked data.
- Added portable profile audit/import with redacted findings and conservative
  zero-right defaults for reusing agent briefs outside their original project.
- Added stable, display, and native agent-name mapping without claiming that a
  host-generated thread label or ID is controlled by Soulmate.
- Added strict retention and cross-context values plus content-free local
  forgetting-attestation receipts for already-absent terminal memory items.
- `soulmate init` now installs the bundled skill into project-scoped Codex and
  Claude discovery paths; explicit `--refresh-skills` updates only marked
  Soulmate-owned copies, while npm installation alone changes no host config.

## 0.0.5

- Added role-scoped memory proposal, review, promotion, rejection, revocation,
  and expiry evidence as part of Soulmate's agent-handoff boundary.
- Kept Coffee as an optional portable preparation skill that can feed the
  existing agent `skills`, brief, and plan flow without adding a second
  subsystem.
- Added one-item append-only JSONL ledgers with hash-linked transition events
  referencing operator-owned project files without copying their contents.
- Kept the runtime dependency-free with no Coffee CLI or configuration
  subsystem, daemon, database, embeddings, telemetry, provider client,
  background expiry, or external tool execution.

## 0.0.4

- Registry onboarding and documentation release: npm installation is now the
  primary quick-start path, with the tagged GitHub source retained as a
  fallback for source-based installs.
- Kept dotagents and Codex/Claude hooks optional and advanced; no feature or
  dependency changes.
- Normalized the npm executable path so publication no longer needs manifest
  correction.

## 0.0.3

- Simplified standalone onboarding: install the pinned GitHub release, run
  `soulmate init`, and render a first brief without dotagents or hooks.
- Clarified that Node.js 20+ is the sole runtime requirement and moved dotagents,
  host hooks, and model bindings into optional or advanced integrations.
- Updated `init` next-step output to point to the standalone first-value command.

## 0.0.2

- Added explicit project-local Codex and Claude hook plan/apply/status/remove
  commands with idempotent, per-file atomic, conflict-safe settings merges.
- Added the internal `hook-run` bridge for bounded SessionStart and
  SubagentStart context; it is fail-open when Soulmate is unavailable.
- Added a pre-install hook-protocol handshake and five-second native hook
  timeout so stale commands or a stuck bridge cannot become a startup bottleneck.
- Preserved host settings, rejected malformed or escaping hook paths, and
  documented the bounded multi-file rename limitation.

## 0.0.1

- Added the dependency-free, plan-only CLI for config checks, briefs, plans,
  profiles, onboarding, and opt-in receipt verification.
- Added a valid Agent Plugins v1 portable bundle with role-specific skills;
  preserved SessionStart/SubagentStart host-hook resources are separated under
  a reverse-domain extension namespace and are not activated by installation.
- Added explicit runtime binding metadata without model routing, invocation,
  credentials, or implicit fallback.
- Receipts now contain selected config/profile bytes and requested runtime
  metadata only; task and goal fingerprints are not stored.
- Memory fields remain declarations of policy intent only; no storage,
  promotion, expiry, deletion, or revocation is performed.
