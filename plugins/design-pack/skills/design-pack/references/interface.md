# Digital product, service and interface design

## Discover the task and service context

Map entry → intent → action → feedback → recovery → completion or exit. Include off-screen service steps where relevant: messages, staff actions, waiting, support and fulfillment. Record who owns each handoff and what happens when it fails. Existing analytics or research can guide hypotheses; absent evidence is not permission to invent user behavior.

Define the primary user outcome and preservation conditions before changing structure. For a new flow, sketch the sequence with content and state labels before decorating it. For an existing flow, inspect and reproduce the current pain point where possible.

## Inventory meaningful states

Cover only applicable states, but do so explicitly: default, hover, focus, pressed, selected, disabled with reason, pending, success, empty, partial, stale/offline, invalid input, permission denied, destructive action, retry and undo. Each action needs a truthful result and next step. Do not show fake success because a request was merely sent.

Use semantic controls and clear names. Preserve expected keyboard behavior, visible focus and logical focus movement. A modal needs a coherent focus lifecycle. Forms need persistent labels and errors attached to the affected field; include how to recover without losing work. Do not communicate status by color alone.

For chat/editor experiences, check composition input (including CJK IME), text selection, multiline editing, streaming updates, scroll anchoring and whether automatic scrolling steals the user's position. These are behavioral checks, not a mandate to reproduce any branded UI.

## Layout and design system

Use realistic content and worst-case content: long labels, translations, empty records, large numbers, missing images and slow data. Plan hierarchy at content level, then component level. Specify semantic tokens, variants, state changes and responsive rules in editable form.

Responsive work includes narrow viewports, touch targets, landscape, zoom, keyboard overlays, safe areas and content reflow. Mobile is an interaction context, not just a scaled desktop canvas. Preserve task access when navigation collapses. Use progressive disclosure only if the hidden content remains discoverable.

Review density against actual use. A frequent operator may need visible information that a first-time customer does not. Use clear grouping and alignment before decorative containers. Preserve established conventions where they reduce effort; change them deliberately when the brief justifies it.

## Prototype and implement

Make the riskiest flow usable at representative fidelity. Compare alternatives with the same task and content. Choose realistic acceptance checks, for example “recover from a failed booking without losing the chosen date” instead of “looks modern.” Distinguish designer inspection from usability research with participants.

Handoff includes component anatomy, states, semantic behavior, responsive rules, copy, tokens, asset licenses, interaction timing, data assumptions and error/recovery behavior. Track differences between specification and implementation. When implementing, preserve framework and component conventions unless a change is required by the outcome.

For service design, deliver a journey and service blueprint: user actions, visible touchpoints, backstage responsibilities, systems, handoff criteria, failure recovery and measures. Prototype scripts or service encounters before expensive rollout, when authorized. A screen-level improvement does not prove service success.

## Maintenance

Name the source of truth for tokens, copy and component behavior. Record deprecated variants and migration effects. When a new request changes scope, update the brief and state inventory, then rerun affected task checks. Maintain known accessibility and device gaps rather than declaring them fixed by redesign alone.
