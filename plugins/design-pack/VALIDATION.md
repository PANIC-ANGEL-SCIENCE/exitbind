# What the checks establish

This is a skills-only source candidate. Checks protect distribution and declared evidence; they do not prove visual quality, actual tool execution, rights, host activation or user acceptance.

## Package integrity

Run from this package root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/check_pack.py --self-test
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_check_run.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_check_image_run.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_check_outcome.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_age_casting_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_attitude_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_expression_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_capture_styling_guidance.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_mirror_selfie_guidance.py
python3 scripts/package.py --output /absolute/private/output/design-pack.zip
```

The package checker enforces:

- Exact file inventory, ordinary UTF-8 files and no symlinks.
- Shared plugin identity/metadata/version, matching host interface metadata and README version.
- Unique JSON keys, finite JSON values and blank machine-readable private templates.
- Confined local Markdown routes, reachability and direct production/discovery routes.
- Compact entrypoint and judgment resource bounds, original license notice, and no structural external locators except the manifest schema.

The authoring convention is intentionally narrow. Skill frontmatter has exactly two single-line fields: literal unquoted name and a description encoded as a nonempty JSON double-quoted string. Documentation uses single-line inline relative links. Reference-style links, multiline destinations/labels, HTML comments and anchor tags are rejected. The compact entrypoint does not allow code markup. This checker is not a general YAML or Markdown validator.

Mutation self-tests corrupt inventory, routes, metadata, template blankness, path confinement, resource bounds and license retention. Their success means the asserted refusals occurred. Tests of routes cannot prove that a maker reads them or follows their meaning.

The packager refuses existing output paths and archives inside the package tree. It validates first, writes a deterministic allowlist and compares every archive member with current source bytes. Rebuilding twice and comparing bytes checks reproducibility in that environment. Nothing is installed or published.

## Private evidence receipts

The optional [screen run](skills/design-pack/templates/screen-run.json) and [image run](skills/mockup-image/templates/image-run.json) belong in the private project. Remove explanatory event shapes before recording observations. Blank templates fail as completed runs.

```sh
python3 scripts/check_run.py --project-root /absolute/private/project --evidence screen-run.json
python3 scripts/check_image_run.py --project-root /absolute/private/project --evidence image-run.json
```

Screen receipts are an optional format for source-informed raster work, not for every original concept. They require each recorded direction decision to cite a viewed source; do not invent references just to fit the format. They check declared maker/view order, local confinement, raster signatures and hashes, decision/reference associations and review of the latest render. Ordinary mode permits iterations; frozen mode permits one complete-screen render. It checks signatures, not full image decoding.

Image receipts bind the saved prompt and declared image inputs to call arguments, verify local hashes and PNG/JPEG dimension headers, compare native aspect claims and require review of the latest output. Only frozen-trial mode requires an immediate first-output freeze. Ordinary mode permits the legacy first-output hash marker but does not require it or suspend iteration; each declared output still must match its retained local bytes. Header checks are not full decoding. Source formats unsupported by this optional checker do not prevent using an otherwise supported production tool; retain the original and identify any supported receipt snapshot derivative honestly.

`required_reference_seqs`, when present on a generation, lists earlier viewed references that must reach actual image inputs. It can be empty when viewed images were only optional visual study. Missing required inputs need `reference_omission_approval`; its text is an attestation, not authorization proof. Records without this field retain the earlier text-only omission rule using `text_only_approval`. These optional fields distinguish visual research from mandatory provider conditioning and detect partial required-input omissions. They do not infer what the user required.

Input roles, tool IDs, order, approval statements and opaque image mappings are declared evidence, not trusted execution logs. The checkers cannot prove perception, conditioning inside a model, actual freeze timing, fidelity after a generative edit or live interaction. Honest unresolved defects may pass evidence checks. No taste threshold or mandatory evidence ritual is added.

## Optional scoped outcome gate

The [outcome review](skills/design-pack/templates/outcome-review.json) is a separate private record, not an extension that silently changes the meaning of a screen/image receipt. Use it when a task benefits from explicit readiness checks. It supports original work without reference receipts. Blank templates fail as completed records. The executable synthetic fixtures in [outcome tests](scripts/test_check_outcome.py) illustrate the schema; they are not production evidence.

```sh
python3 scripts/check_outcome.py --project-root /absolute/private/project --evidence outcome-review.json
```

- `contract_source` identifies the current task instruction. `requirements` use unique IDs, an instruction, a category (`shape`, `relational-identity`, `sequence`, `continuity`, `language` or `other`) and exact `targets` containing `artifact_id` and `location`. Shape requirements also state the requested `shape`. No shape or culture is inferred by the checker.
- `artifacts` have unique IDs, local `path`, `sha256`, `kind` (`raster` or `pdf`) and positive declared `produced_seq`. Raster signatures and file hashes are checked. PDF signature/hash checking does not parse, render or validate the document. Renders remain required for visual observations.
- `views` bind an artifact ID/hash to a later declared `seq`, `evidence_type: image-view`, `view_tool_result_id` and `scale` (`overview`, `use` or `detail`). Tool IDs and event timing are self-reported, not authenticated execution.
- `checks` cover every scoped requirement/target exactly once, using the same ID/location, a `result` (`pass`, `fail`, `not-tested`), concrete `observation` and relevant `view_ids`. Untested checks record the gap and may have no view. A shape observation includes `contour_observation` and `affordance_observation`; passing explicit circles/capsules also supply finite `geometry` width, height, unit and four `effective_corner_radii`. These are actual used circular radii, not declared CSS values or approximate screenshot guesses. The test checks their mathematical consistency, not pixels. Other requested shapes use their visual observations; the checker does not pretend to measure arbitrary contours. Relationship checks separate `observed_relationship`, `interpretation` and `alternative_reading`; their honesty and freedom from stereotypes still need human/model judgment.
- Optional `sequence` names `ordered_pages`, a separate `overview_id`, ordered `overview_members` with final page hashes, `format` (`pdf` or `raster-series`), and explicit `sequence_first`. When sequence-first work is chosen, `plan` supplies earlier sequence number, page order, argument, ordered page contributions, transition, `study_artifact_id` and `study_view_id`. In ordinary work, the separate raster study must have been declared viewed before final-page production (and before final PDF production for a PDF). Explicit frozen trials retain planning-only alternatives under the first-pass protocol and therefore do not require a rendered study. It is not required for unrelated artifacts. PDF sequences also identify `pdf_id`/`pdf_page_count`, and each page's `derived_from` binds the final PDF hash and page number. The declaration cannot prove the raster was actually rendered from that PDF; use the actual rendering tool and retain its evidence.
- The sequence `review` records `result`, `view_ids`, `hierarchy`, `transition`, `continuity` and `reading_size`. Tested reviews require all current pages at use scale plus their overview. Non-PDF series and arbitrary page counts are supported. Changing bytes invalidates the bound views; changing order invalidates the plan/overview association.

`status: rework` permits honest failed or untested outcomes, including `mode: frozen-trial`; it never authorizes edits. `ready-for-user-review` requires every scoped result and any sequence review to pass. It is not user acceptance. A caller can omit a real requirement or write a false observation: the checker cannot infer missing brief scope, perceive beauty, establish cultural membership, authenticate input authority or evaluate generated imagery. Read the actual brief and pixels. Successful gate tests demonstrate declared-record acceptance/refusal only, not better designs.

## Manual and runtime verification

Inspect exact source and archive for private names, project identifiers, source links and filled Markdown templates. The locator scan cannot recognize every private name in prose; blank JSON checks do not establish all free-form templates are unfilled. Keep any private denylist outside the pack.

Use [evaluation scenarios](EVALUATION.md) with actual artifacts when behavioral evidence is needed. Compare requested actions with visible state changes, clean source detail with revisions, and benchmark pixels with the output at equivalent scales. Check accessibility and truthful states separately. A central craft failure remains unresolved even if structure and functional requirements pass; reviewer approval is not owner acceptance. Host installation/discovery and production integration require observation in the intended host. A version bump, archive hash or planning exercise is not a demonstrated aesthetic improvement.

## Model-direction route coverage

Version 0.7.8 adds direct routes from the mockup entrypoint, image production and generation contract to model direction. Four additional mutation cases remove each route or the resource and require structural rejection. They prove route/inventory enforcement, not that a maker inspected sources or that casting, expression, styling, diversity or phone texture succeeded. The model-direction scenarios in EVALUATION.md require separate actual-pixel trials; no live generation or owner acceptance is claimed by package checks.

## Age-aware casting coverage (0.7.9)

Run `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_age_casting_guidance.py` for static contract assertions over the model-direction text, brief fields and synthetic scenario coverage. Its mutation checks remove each policy passage and verify that the corresponding assertion fails. These are documentation regression tests, not a language-model evaluation, age classifier, image-quality gate or completed scenario execution. They cannot verify that a model obeys the instructions or that an actual person appears a certain age. No new receipt fields are mandatory. No images were generated for this candidate by these static tests; visual trials and intended-host loading require separate evidence.

## Correction and attitude coverage (0.7.10)

Run `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_attitude_guidance.py` for written-contract regressions and synthetic declared-outcome cases. Text mutations verify that key safeguards cannot disappear unnoticed: preserving successful cues across age corrections, independent attitude review, no profession or ethnicity inference, no costume or hard-stare shortcut, and no automatic aesthetic success. These tests check the presence of guidance, not its execution by an image model.

The outcome cases use a synthetic image-signature fixture, not a portrait or an age/attitude classifier. Independently declared passes for age fit and capture do not make a failed or untested attitude requirement ready. Honest rework is accepted; false readiness is rejected. The existing optional outcome schema is unchanged, and no receipt ritual becomes mandatory. Omitting a real requirement or falsely declaring a pass can still evade the checker, so actual brief and pixel review remain necessary.

The [attitude scenarios](EVALUATION.md) describe behavioral trials to run with permitted images. They are not executed image trials or proof that a person belongs to a creative profession. Keep actual prompts, references, outputs and user decisions private and report them separately. No generation is performed by the package tests; no visual improvement, owner acceptance, host activation, installation or publication follows from passing them.

## Expression calibration coverage (0.7.11)

Run `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_expression_guidance.py` for written-contract regressions and synthetic declared-outcome cases. The text tests protect guidance about facial cue stacking, expression/attitude separation, correction scope, no universal neutral face, and actual-pixel review. Mutations remove each passage and verify that its contract fails. They do not analyze prompt semantics, detect expressions in images or establish that a maker follows the method.

The synthetic outcomes add a task-scoped expression-fit requirement to the existing optional schema. Declared excessive expression, untested expression or loss of attitude after a quieter-face correction blocks readiness; honest rework remains valid. A declared small-smile or expressive-brief pass is not prohibited. These are record-logic tests, not facial classifiers or executed image trials. A false pass or omitted task requirement can evade them. Review actual references, current output and receiving crops, and keep visual findings and user acceptance separate.

The same expression test module also protects the separate photographic-surface review: evidence-led diagnosis of visible artifacts, no clinical interpretation, no blanket smoothing/blur/grain, and preservation of accepted layout/composition. Its synthetic surface-fit declarations exercise the unchanged outcome gate independently of expression. They are not image-artifact detectors and do not establish why a viewer dislikes a generated person.

## Framing, lighting and worn styling coverage (0.7.12)

Run `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_capture_styling_guidance.py` for written-contract deletion guards, direct routes, brief fields and synthetic scoped outcomes. The package self-test also removes the method, its test file and each of four direct routes. These are document/inventory checks, not camera, lighting or fashion classifiers. They cannot determine whether a photograph looks staged, estimate a real light source or assess a garment from pixels.

The existing optional outcome schema is unchanged. Synthetic framing, lighting and styling requirements independently reject a ready declaration if any is declared failed, untested or missing; honest rework remains valid. Explicit catalogue framing, clean light and simple clothing are not globally rejected. A false observation or omitted real task requirement can still pass. No live generation is run by these tests, and they cannot authenticate guidance reads or generator calls. Actual source inspection, submitted calls, output review and user acceptance require separate evidence. No installation, publication, automatic activation or causal quality improvement follows from these checks.

## Mirror capture and face/body consistency coverage (0.7.13)

Run `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_mirror_selfie_guidance.py` for written-contract deletion guards, direct routes, brief coverage and synthetic declared outcomes. Package self-tests also remove the method, test file and four direct routes. These are text/record checks, not ray tracing, hand-anatomy detection or age estimation.

Using the unchanged optional outcome schema, declared mirror-mode, hand, geometry, light and requested life-stage findings remain independent. Failed, missing or untested scoped requirements block a ready declaration; honest rework remains valid. Centred framing, an unobscured face and no-flash lighting are not globally rejected. False observations or omitted real requirements can evade the checker. No mirror image is generated by these tests, and they cannot prove that guidance was read before generation. Actual read/call records and current-pixel reviews are separate evidence; neither a version bump nor a fixture establishes automatic activation, visual success or user acceptance.

## Whole-scene reset and section synthesis coverage (0.7.14)

The existing `test_capture_styling_guidance.py` additionally checks written reset/repair, organized-source and coupled-scene contracts, brief routes and iteration boundaries. These are text deletion guards, not identity comparison, outfit recognition, room classification or scene-coherence scoring. They cannot inspect hidden prompt conditioning or prove that the maker used the new guidance. Fictional-person checks protect the written boundary between provisional motives, visible consequences and unsupported biography; they do not assess a person’s interests, occupation or psychology.

The unchanged optional outcome schema supports task-scoped reset-subject, reset-outfit, reset-space and scene-coherence declarations. Synthetic cases show that declared failures, missing checks or untested results cannot be marked ready; individual passes do not override a failed whole-scene requirement. Honest rework and retained explicit constants remain valid. False observations or omitted real requirements can still pass. No new photograph is generated or reviewed by these tests. Actual organized-source inspection, submitted inputs, changed artifacts, reviewer evidence and user acceptance must be recorded separately. A reviewer or tool gate cannot supply the user's OK, and a test result cannot retroactively establish governance.

## Supporting artifacts and light-response coverage (0.7.15)

The existing capture-styling tests protect written contracts for inspectable supporting masters, scoped visible transfer, exactness/legibility limits and independently assessed illumination, exposure and capture response. Deletion guards and synthetic scoped outcomes check documentation and declared-record logic only. They do not create a print, compare a scene to a source, recognize text, estimate illuminance or recover camera metadata.

Using the unchanged optional outcome schema, declared artifact-master, depiction-fit, legibility-fit, exposure-fit and capture-response findings remain independent. Failed, missing or untested required findings cannot be marked ready; honest rework remains valid. No-text graphics, coherent unreadable small fragments and clean low-light pictures can be valid when the brief permits them. False observations or omitted real requirements can evade the gate. Actual master creation/viewing, submitted inputs, returned pixels, visual review and user acceptance require separate evidence. These tests do not demonstrate activation, exact transfer or aesthetic improvement.

## Reference-fidelity repair coverage (0.7.16)

The existing capture-styling tests protect written source-specific selection, production transfer, construction/material diagnosis, visibility, non-stereotyped recasting and actual comparison boundaries. Exact text deletion guards are not semantic comprehension tests. The unchanged optional outcome schema can keep reference-fashion-fit, construction-fit and garment-visibility separate from casting and capture. Synthetic failed, missing or untested declarations block readiness; honest rework and authorized simple clothing remain valid.

These tests do not compare real outfits, identify brands, price garments, validate manufacturing or estimate a person's age. They cannot detect a false observation or an omitted true requirement. Actual reference/output pixels, applied production inputs, correction scope and user acceptance remain necessary; no visual improvement or automatic plugin use follows from a pass.

## Material and garment-finish coverage (0.7.17)

Existing capture-styling tests protect written diagnosis of apparent material behavior, support-to-finish continuity, cropped-reference limits and narrow repair protection. Synthetic task-scoped material-response, garment-continuity and protected-region findings remain independent in the unchanged optional outcome schema. Failed, untested or missing requirements block readiness; honest rework and a deliberately quiet hem remain valid.

These are text and declaration checks, not cloth simulation, sewing inspection, material identification or pixel-level preservation tests. They do not prove that a generated garment is manufacturable or that hidden stitches exist. False observations or omitted real requirements can still pass. Actual source/output comparison, visible finish and footwear contact, changed/protected regions and user acceptance need separate evidence.

## Task-conditioned subject and framing coverage (0.7.18)

Existing capture-styling tests protect written distinctions among portrait, outfit-led, wearer-view and object-only modes, complete-source framing evidence, applicable person checks and preserved required content. Synthetic optional outcomes can require subject-mode, framing-evidence and content-evidence without irrelevant face/age fields. Failed, missing or untested scoped requirements block readiness; intentional face exclusion can be valid only when the task does not require a face.

These are lexical and declared-record tests, not image classification, crop detection, selfie authentication or object recognition. They do not establish what a real reference contains or whether an output actually follows its mode. False observations or omitted true requirements can pass. Actual source/output inspection and user acceptance remain necessary; no new image was generated by these checks.

## Individual presentation and purposeful pose coverage (0.7.19)

Existing capture-styling tests guard written source-group overlap, identity/expression separation, task-selected body views, meaningful pose change, and fictional personal-detail consistency. Synthetic optional declarations separately require pose-distinction, presentation-fit and personal-detail-fit. Failed, missing or untested declared requirements block readiness; a valid back view or absent tattoo is not globally rejected.

These are lexical guards and declared-record logic, not gender classification, pose recognition, story verification or tattoo detection. The tests do not infer identity or validate an image, and no image generation runs as part of them. A dishonest pass or omitted true requirement can still pass. Actual source/output inspection, applicable use authority and user acceptance remain separate.

Group declarations additionally exercise requested people count, individual distinction and contact coherence independently. They do not count actual bodies, authenticate relationships or enforce physical anatomy. Two-person and larger groups are task-specific valid declarations, not a new minimum or default.

## Coherent-subject repair coverage (0.7.20)

Existing capture-styling tests protect written distinctions among perceptual casting fit, head/neck/shoulder-body relationships, pose/projection/occlusion and capture-response continuity. Synthetic optional outcomes declare subject-continuity, capture-continuity and protected-regions separately. A failed, missing or untested scoped requirement blocks readiness; an intentionally obscured profile, raised supporting shoulder or shaded face is not globally rejected.

These are lexical guards and synthetic record checks, not anatomical measurements, gender classification, face/body matching or visual preservation tests. They do not authenticate an edit or establish that a user-reported mismatch was solved. A falsely declared pass can pass; actual pixel comparison and the user's judgment remain necessary. No live image generation runs in these tests.

Optional body-load, tattoo, accessory, palette and makeup declarations can fail independently; absence of an unrequested adornment or cosmetic is not a mandatory field failure. Written guards protect task-specific source observation and transfer boundaries, not actual skin deformation, attachment, color matching, cosmetic detection or anatomical correctness.

## Coupled outfit-intent coverage (0.7.21)

Existing capture-styling tests guard material/silhouette/layer/wear relationships, user-specified context versus inferred cultural facts, palette-only substitution, cropped-source uncertainty and reopened fictional details. Synthetic optional requirements separately declare outfit-intent, personal-detail-coherence, graphic-transfer and protected-regions. Failed, missing or untested scoped requirements block readiness; an authorized reinterpretation or intentional contrast is not globally rejected.

These checks are written-contract deletion guards and synthetic declared-outcome logic, not fashion classification, cultural-authenticity scoring or biography verification. They do not prove source consumption, material recognition, actual master replacement or visual coherence. A falsely declared pass can pass. Actual source/output inspection and user acceptance remain necessary; no live generation runs in these tests.

## Request reconciliation and private-profile coverage (0.7.22)

Written-contract tests cover current/superseded/optional requirements, inaccessible-history limits, writing-style precedence, private guide consumption and project-defined content semantics. Synthetic optional records declare request-coverage, style-fit, item-semantics and preserved-requirements separately when the synthetic task requires them. Failed, missing or untested scoped checks block readiness; a photo-only task need not claim copy or implementation evidence.

These tests protect written guidance and declared outcome logic. They do not retrieve full conversations, execute a profile resolver, authenticate a guide read, classify copy style or test an app's actual item navigation. A false pass or omitted real requirement can pass. Read the actual project sources and inspect the delivered output; unavailable history and untested runtime integration remain explicit. No live generation, external publication or installation runs in these tests.

## Garment structure substitution coverage (0.7.23)

Written-contract guards cover pre-generation construction reasoning, generic color/category fallback, visible support-to-edge continuity, private source boundaries and simple-garment counterexamples. Synthetic optional outcome records separate garment-silhouette, garment-construction and preserved-regions; missing, failed or untested required declarations block readiness. These are lexical and declared-record tests, not pattern reconstruction, material simulation or visual fashion scoring. A false pass or omitted real requirement can pass. Actual references, call inputs and output pixels still require inspection; no live generation or demonstrated aesthetic gain is claimed.

Private-fashion tests cover written loading/precedence/status boundaries and synthetic declared profile-read checks. They do not implement or exercise a real host loader, access private accounts, authenticate a read or establish downstream execution. Actual runtime profile integration remains separately untested.
