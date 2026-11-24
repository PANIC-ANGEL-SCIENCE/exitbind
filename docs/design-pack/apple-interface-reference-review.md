# Interface source-role review

## Sources and limits

- [Apple HIG: Materials](https://developer.apple.com/design/human-interface-guidelines/materials) distinguishes a functional navigation and control layer from content, and discusses contrast and choosing materials to suit that separation. Transfer the hierarchy, not a branded shader, a universal glass surface or a fixed blur radius.
- [Apple HIG: Accessibility](https://developer.apple.com/design/human-interface-guidelines/accessibility) and [Sidebars](https://developer.apple.com/design/human-interface-guidelines/sidebars) provide platform examples for adapting to access preferences and organizing navigation in the available space. Translate the consequences to the destination's own semantics, input and requirements.
- [Meta's Astryx introduction](https://astryx.atmeta.com/blog/introducing-astryx) separates system behavior and accessibility from project theme customization. The [pinned Astryx review](astryx-reference-review.md) examines specific implementation and guidance rather than treating the introduction as runtime validation.

## Distribution audit

The reviewed Design Pack source tree did not designate a competing vendor system, typeface or framework as a default. This change names preferred references by decision and adds a guard against silently adopting another system when a brief is incomplete. Generic optical and physical materials guidance remains legitimate. The Design Pack distribution references neither private calibration nor proprietary assets and requires no external package, font or toolkit for these methods.

The checks exercise written contracts and packaging, not actual assistive technology, visual quality or installer discovery.
