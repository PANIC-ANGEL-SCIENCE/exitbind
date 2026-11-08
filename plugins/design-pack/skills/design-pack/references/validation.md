# Critique, accessibility and evidence

Start with the artifact and brief. Record version, date, output medium, relevant environment and limitations. For an existing digital product, inspect real screens and interactions before making visual findings. A source-only review must be labeled as such.

## Two independent questions

1. **Does it achieve the agreed purpose?** Task or communication success, the requested expressive ambition and benchmark relationships, audience/context fit and actual preservation requirements.
2. **Does the medium work correctly?** Accessibility, state behavior, responsiveness, production fidelity, rights, performance and operational usability.

Aesthetic preference is not proof of task failure, but an explicit art-direction or craft goal is part of the task. A technically valid artifact can still fail that goal. Assess both without averaging a severe failure into a passing score. Preserve user requirements; do not mistake preserving historical evidence for preserving a rejected designer-selected solution.

## Evidence matrix

For each criterion, record: requirement ID, current artifact/version, method, conditions, observation, result (`pass`, `fail`, `not-tested`, or `not-applicable` with reason), and evidence location. Never reuse evidence from an altered artifact without checking whether the relevant inputs changed.

Screenshots establish appearance in a captured state, not behavior. Automated tests establish the assertions they ran, not all user intent. A checklist is a plan until its items are observed. Synthetic exercises do not establish production-host compatibility.

## Digital accessibility baseline

Use the project's agreed accessibility target. In the absence of one, propose WCAG 2.2 AA as a web baseline; do not claim legal or complete conformance from this short list. Check the current standard for applicability and exceptions.

Check keyboard operation and focus visibility/order; meaningful structure and control names; text and non-text contrast; text resizing and reflow; target sizing; error identification and recovery; alternatives for images/media; reduced motion and pause controls; status information that does not rely only on color. Inspect actual rendered states. Use automated checks plus manual tasks and relevant assistive technology when available; record untested browsers, devices and AT.

Use semantic regions and headings that reflect content relationships. Prefer platform-native semantics; adding ARIA does not repair an incoherent web interaction. For native mobile work, choose the actual platform target and test its accessibility tree, VoiceOver or TalkBack navigation/activation, focus and return behavior, dynamic text, touch targets and relevant system settings. A photo-triggered operation needs an accessible activation and non-spatial route to its objects. Static boards cannot validate native accessibility; name it as untested until exercised in the implementation.

## Context matrix

- **UI:** representative narrow and wide views, zoom, keyboard, touch where available, long/localized content, loading/empty/error states, recovery and preservation.
- **Print/editorial:** actual scale, proof, dimensions, assets/fonts, reading sequence and export/preflight; production requirements are printer-specific.
- **Motion/audio:** full playback, critical frames, captions, sound mix, flashes, timing and target output; document if listening was unavailable.
- **3D:** required views, scale, structure, materials, movement, target export and performance; distinguish hidden-geometry assumptions.
- **Physical/spatial/fashion:** appropriately measured prototypes, human context and specialist validation. Visual mockups cannot establish physical safety or fit.

Choose a finite set based on the brief and risk. Do not invent a universal device matrix or run unrelated checks just to accumulate evidence.

## Direction and content check

Compare the rendered work with the brief and the observed reference relationships. Identify unsupported combinations, generic substitutions and decorative content that competes with the task. Apply the line-by-line copy role, deletion and swap tests in [reference-analysis.md](reference-analysis.md). Verify that counts, labels and promises reflect real content or clearly marked samples. Keep task behavior, directional fit and client acceptance as separate results; a passed browser test is not aesthetic approval.

Use the visual rejection criteria in [visual-methods.md](visual-methods.md) to locate lost priority, broken relationships, unsupported concepts, unreadable states, craft defects and effects without benefit. Record a visible counterexample at the actual size; functional passes alone cannot settle these findings. For an explicit [first-pass trial](first-pass.md), verification is read-only: report the frozen artifact's failures without repair or reselection.

For app screens, use the concrete [screen review](screen-production.md) alongside source captures. Identify the weakest visual relationship even if every click works. Aesthetic findings need observed locations and consequences, not a fixed defect count or subjective score threshold.

## Findings that lead to repair

Each finding includes location/state, observation, consequence for this audience, severity, confidence, proposed change and how to verify it. Assign severity by the unmet brief and consequence, not by discipline. Safety/access failures need attention, and a central expression, concept or composition failure can also block a craft-led deliverable. A small tonal difference may be local; an image with the wrong attitude can invalidate the whole direction. Explain the distinction with visible evidence rather than automatically demoting every aesthetic gap to refinement.

When revision is authorized, choose repair or replacement at the scale of the failure and inspect again. Compare with the benchmark as well as the previous version: improvement over rejected work alone is not enough. Stop when agreed criteria are met; if a real blocker prevents completion, name the unfinished outcome instead of declaring it ready with caveats. If local refinements leave the core gap intact, replace the organizing idea within scope. After image or whole-board edits, compare protected regions against clean masters; corrected copy does not offset degraded photographic detail.

## Gate the requested outcome, not only the geometry

Translate concrete feedback into task-scoped checks before making the revision: required control shape, a named content-language policy, a subject/context relationship, or the reading rhythm of a connected deliverable. Check all affected targets and states rather than accepting one successful crop. Keep “observed,” “inferred” and “not tested” distinct. For scene identity, describe the visible relationship and competing reading, not a stereotype or a guaranteed authenticity verdict.

For a connected portfolio, review the final sequence overview, the actual ordered page transitions and each readable page. When PDF is requested, inspect renders from the final PDF as well as technical export checks. A post-export replacement or naming change invalidates affected page and sequence findings. Geometry can refute a claimed capsule but cannot establish affordance, attitude or presentation force.

The optional [outcome review](../templates/outcome-review.json) and checker can bind scoped criteria, artifacts, views and a readiness decision. Honest failed or untested work may be retained as rework; it cannot be called ready by averaging it with technical passes. This is an optional private gate, not a mandatory workflow, independent execution log or algorithmic taste judgment. It does not replace actual inspection or user acceptance.

## Delivery review

Can the recipient open and edit the source? Are export specifications and asset permissions recorded? Are example data and inferred claims labeled? Are approved constraints preserved? Are untested areas stated? Does the handoff explain what is finished and what requires real-world proof?

If an independent review is required by the task, obtain it from an actual separate reviewer. Give the brief, applicable source pixels and artifact, not an instruction to ratify the maker's conclusion. Otherwise label self-review honestly. A reviewer must be able to recommend rework of a technically compliant concept. Formal Exitbind acceptance and user acceptance remain separate states and authorities.
