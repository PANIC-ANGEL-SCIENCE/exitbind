# From reference to composed artifact

Use after [reference-analysis.md](reference-analysis.md) and before production. This is an independently authored decision method, not a style library. The public sources and their observation limits are in [sources.md](sources.md). Everything below can be specified in text without visiting a service or importing a dependency.

## Make a direction executable

Write one compact contract with these fields:

1. **Subject and priority:** name the actual content, the first thing to understand and the next action. Specify what may be secondary. If there is no action, name the intended reading sequence or emotional progression.
2. **Evidence chain:** source/state and evidence type → observed relationship → proposed consequence for this artifact → visible test → rejection condition. An author's capability claim can suggest an experiment; it cannot establish its visual success.
3. **Organizing idea:** explain how content is arranged, not just how it feels. Compare two or three ideas in planning using the same content: sequence versus overview, object versus relationship, dense comparison versus staged focus. Select within delegated authority. For one-shot work these remain notes, never rendered variants; follow [first-pass.md](first-pass.md).
4. **Composition:** describe dominant, supporting and quiet zones; reading order; grid and major proportions; image/crop anchors; type hierarchy and density; edge and overflow behavior. Choose proportions for this artifact and record why. Do not copy a reference's pixel values without its size and task.
5. **Signature:** choose a small number of relationships that make the concept recognizable, such as a route line connecting events or a repeated cut in a material. Show how each connects at least two of layout, type, imagery, interaction and material. A gradient behind an otherwise generic layout is not an organizing idea. No effect is mandatory.
6. **Production contract:** resolve semantic color pairs, assets, motion, states, fallbacks and budgets. Name the weakest assumption and the exact view/state that could disprove it. Carry those tests into [validation.md](validation.md).

## Compose at useful scales

First list content in priority order. Estimate how much of the intended frame or physical surface each role needs. Choose one dominant anchor; give supporting information enough distinction to scan without matching its weight. Deliberate multiple focal points are possible for comparison tasks, but explain their ordering. Avoid fake labels or invented metadata as spacing devices.

Define a grid with real edges: text baselines, image subject, control hit area and container. Check optical alignment at actual size: a circular icon can need a different inset than a square one; a heading with open letter shapes can look misaligned on a mathematically shared edge. Keep hit areas stable while adjusting visible marks. Record intentional exceptions locally rather than distorting the entire spacing system.

Test the same composition as a small silhouette/blur for mass, at reading size for hierarchy, and at target narrow/wide or near/far conditions for content. The silhouette test does not prove readable text. Check line breaks, language coverage, image focal points, awkward tangencies, isolated fragments and crowded edges. A crop that removes evidence about the subject is a content defect even if the shape is attractive.

## Asset and effect decision sheet

For each proposed asset or effect, record **purpose → parameters → placement → cost budget → fallback → rejection test**. Choosing none is a complete decision. Use the sheet for imagery and materials as well as animation.

| Family | Controls to specify | Useful test and reason to reject |
| --- | --- | --- |
| Geometry / SVG | Canvas ratio, count, scale, amplitude, spacing, repetition, crop, clear zones | Inspect edges and target sizes; reject accidental tangencies, false affordances or loss of content space |
| Gradient / light | Color stops, direction, coverage, lightness extrema, layering and text-safe zones | Inspect all text/control locations against the brightest and darkest regions; reject moving or responsive contrast failures |
| Grain / dither / texture | Grain size relative to output, density, opacity, frequency and export resolution | Inspect actual scale and compression; reject muddy small type, moiré, banding or a material claim unsupported by a proof |
| Motion / shader | Trigger, shape, light, speed, amplitude, loop behavior, interruption and stable state | Inspect full cycle and reduced motion; reject distraction from the task, missing keyboard access or a blank fallback |
| Interactive component | Semantic role, input methods, focus, state feedback and recovery | Test keyboard, touch and errors; reject an effect that conceals state, invents an action or breaks native semantics |

Set budgets from the target environment before making the effect: maximum simultaneous attention signals, occupied area near critical text, transfer size and acceptable frame/load cost if digital; ink/material tolerances if physical. Values belong to the project. Do not fill them with arbitrary universal numbers. If runtime measurement is unavailable, retain a static option and mark performance untested. Keep labels and actions usable when imagery fails, motion is reduced, rendering support is absent or an optional effect is disabled. Do not require an external generator to achieve the method.

## Worked example A: community repair timetable

Synthetic brief: a fictional community workshop needs visitors to find a session by time and skill level. Session data is supplied; no attendance or popularity claims are permitted. References suggest contextual color roles (Happy Hues) and controllable geometry (Haikei), based on described capabilities rather than a tested outcome.

**Planning alternatives:** a chronological rail emphasizes when to arrive; a skill-category matrix emphasizes comparison. Choose the rail because this brief prioritizes arrival time. This choice would change if people mostly compared simultaneous topics. Do not combine both into competing primary navigation.

**Composition decision:** times form a stable narrow column, titles form the dominant reading column, and skill labels sit beside their own sessions. A route line connects time groups but stops before controls, so it cannot resemble a progress indicator. At narrow widths, keep time beside its title and let metadata wrap below; do not shrink the entire grid. Use tabular time figures if available and test whether the first digit appears aligned with the rule. Select type based on actual language coverage.

**Pre-render decision card (illustrative values, not pack defaults):** for a 720-unit guide, start with 32-unit side margins, a 72-unit time column and a 24-unit gap, leaving 560 units for session content. At 360 units use 20-unit margins, a 52-unit time column and a 12-unit gap, leaving 256 units for content. A 16-unit body with 24-unit line height and a 28/32 title establishes a testable rhythm; session groups start on a 32-unit gap. There is no image crop because this brief needs no image. The signature is the time-to-session relationship, not a decorative hero. Reject these values if long titles, the language's glyph metrics or required zoom break the association; the decision card must then record the finding or an authorized revision.

**Color/state decision:** quiet canvas and readable text support the schedule; selected session and keyboard focus have separate roles. An expressive accent may mark the selected session, but skill level also has a text label. In [color.md](color.md), test normal, selected, unavailable and focus pairs with the route line present. Large decorative color patches are optional and omitted here because they compete with the timetable.

**Rejection and verification:** reject a line mistaken for a completion state, a time visually attached to the wrong title, lost selection under focus, or a long title displacing its time. Inspect short/long titles, simultaneous sessions, an empty day and keyboard selection in the built artifact. These are planned tests, not claims that this fictional design has been rendered or validated. In one-shot mode, record failures against the frozen result instead of repairing it.

## Worked example B: traveling geology exhibition

Synthetic brief: an exhibition introduces rock strata through a poster, a mobile visitor guide and a projected opening scene. Supplied specimen photography has permission; geological claims must come from supplied captions. The concept is layered time, not a generic premium mood.

**Planning alternatives:** a specimen atlas makes comparison primary; a layered cross-section organizes a chronological story. Choose the cross-section for the commissioned sequence. The atlas remains appropriate for a scientific comparison task. Dark's categorized gallery suggests testing dark surface hierarchy, not choosing dark by default. Haikei's geometry controls suggest independently drawn layer boundaries; no generated asset is bundled.

**Composition decision:** use one specimen crop as the anchor and uneven layer divisions to relate sections. Keep captions on stable reading fields. The poster emphasizes a title and location; the mobile guide emphasizes section labels and reading order. Repeated layering connects image boundaries, headings and transitions without turning every button into a rock shape. Inspect whether the crop preserves identifying features and whether edges accidentally slice letters.

**Effect decision:** a slowly shifting light could introduce the projection's strata; ShaderGradient's described light/motion controls supply parameters to explore, not proof of suitability. Set the projection's own motion and brightness limits. The printed poster and reduced-motion guide use a static field. Grain is optional: compare its purpose to the actual mineral texture, and omit it if it obscures the specimen or merely duplicates detail. Grainient is a catalog reference, not an asset license. Interactive guide controls retain semantic labels and focus irrespective of React Bits-inspired transition experiments.

**Rejection and verification:** reject unreadable captions at projection distance, a dark treatment that hides specimen boundaries, texture that changes the apparent evidence, or chronology conveyed only by motion. Inspect poster proofs, narrow guide state, the full opening cycle and static fallback as appropriate; until observed, these remain test obligations. Concept, legibility, rights and runtime performance each receive their own finding.

## Visual rejection criteria

Set brief-specific thresholds before inspection. Use visible observations rather than a numerical taste score:

- **Priority lost:** secondary decoration or metadata wins the first glance; the intended subject/action cannot be located at the target size.
- **Relationship broken:** a shared axis, grouping, scale or sequence implies the wrong association; responsive adaptation destroys it.
- **Concept unsupported:** the signature depends on invented facts, a mood label or an asset that cannot be used; removing it leaves no subject-specific organizing idea.
- **State unreadable:** palette roles collapse in selected, error, focus, image or moving-background conditions; function depends only on color or motion.
- **Craft breakdown:** unintended crop, collision, tangent, orphan or optical imbalance interrupts the intended hierarchy at real size.
- **Effect without benefit:** removing an effect preserves the intended communication and improves clarity or reliability. Keep it only if a commissioned expressive purpose still justifies the cost.

A rejection identifies artifact/version, location/state, observation, consequence and next test. It is a finding, not permission to revise a frozen first pass or override a client's deliberate choice.
