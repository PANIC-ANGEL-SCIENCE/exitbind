# Critique, accessibility and evidence

Start with the artifact and brief. Record version, date, output medium, relevant environment and limitations. For an existing digital product, inspect real screens and interactions before making visual findings. A source-only review must be labeled as such.

## Two independent questions

1. **Does it achieve the agreed purpose?** Task or communication success, concept coherence, audience/context fit and preservation requirements.
2. **Does the medium work correctly?** Accessibility, state behavior, responsiveness, production fidelity, rights, performance and operational usability.

Aesthetic preference is not proof of task failure. A technically valid artifact can still express the wrong idea. Assess both without averaging a severe failure into a passing score.

## Evidence matrix

For each criterion, record: requirement ID, current artifact/version, method, conditions, observation, result (`pass`, `fail`, `not-tested`, or `not-applicable` with reason), and evidence location. Never reuse evidence from an altered artifact without checking whether the relevant inputs changed.

Screenshots establish appearance in a captured state, not behavior. Automated tests establish the assertions they ran, not all user intent. A checklist is a plan until its items are observed. Synthetic exercises do not establish production-host compatibility.

## Digital accessibility baseline

Use the project's agreed accessibility target. In the absence of one, propose WCAG 2.2 AA as a web baseline; do not claim legal or complete conformance from this short list. Consult the [W3C quick reference](https://www.w3.org/WAI/WCAG22/quickref/) for applicability and exceptions.

Check keyboard operation and focus visibility/order; meaningful structure and control names; text and non-text contrast; text resizing and reflow; target sizing; error identification and recovery; alternatives for images/media; reduced motion and pause controls; status information that does not rely only on color. Inspect actual rendered states. Use automated checks plus manual tasks and relevant assistive technology when available; record untested browsers, devices and AT.

W3C's [page structure tutorial](https://www.w3.org/WAI/tutorials/page-structure/) supports semantic regions and headings that reflect the content relationships. Prefer native semantics; adding ARIA does not repair an incoherent interaction.

## Context matrix

- **UI:** representative narrow and wide views, zoom, keyboard, touch where available, long/localized content, loading/empty/error states, recovery and preservation.
- **Print/editorial:** actual scale, proof, dimensions, assets/fonts, reading sequence and export/preflight; production requirements are printer-specific.
- **Motion/audio:** full playback, critical frames, captions, sound mix, flashes, timing and target output; document if listening was unavailable.
- **3D:** required views, scale, structure, materials, movement, target export and performance; distinguish hidden-geometry assumptions.
- **Physical/spatial/fashion:** appropriately measured prototypes, human context and specialist validation. Visual mockups cannot establish physical safety or fit.

Choose a finite set based on the brief and risk. Do not invent a universal device matrix or run unrelated checks just to accumulate evidence.

## Findings that lead to repair

Each finding includes location/state, observation, consequence for this audience, severity, confidence, proposed change and how to verify it. Rank: blocking task/safety/access failure; major comprehension/coherence failure; local usability defect; craft refinement. Explain context rather than assigning numerical “taste” scores without a rubric.

After repair, inspect the affected context again. Stop when agreed criteria are met or remaining gaps are explicit and correctly owned. If repeated refinements do not improve the result, revisit the brief or concept instead of endlessly adjusting pixels.

## Delivery review

Can the recipient open and edit the source? Are export specifications and asset permissions recorded? Are example data and inferred claims labeled? Are approved constraints preserved? Are untested areas stated? Does the handoff explain what is finished and what requires real-world proof?

If an independent review is required by the task, obtain it from an actual separate reviewer. Otherwise label the review as self-review. Formal Exitbind acceptance is a separate state and authority.
