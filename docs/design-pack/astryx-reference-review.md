# Component and icon method source review

Reviewed 2026-11-12. This is a repository maintenance note, not part of the skills-only archive or evidence of improved generated UI.

Reference: [Meta Astryx](https://github.com/meta/astryx/tree/09dafe8504dd574efe214fc463e5fc9902708a73), exact inspected commit `09dafe8504dd574efe214fc463e5fc9902708a73`. The README describes a beta React/StyleX design system. Its implementation and knowledge records informed original framework-neutral Design Pack methods; no library adoption, asset import, upstream code execution or private calibration was needed.

## Evidence and transfer

All links pin the inspected source rather than a moving branch.

- [Icon role and artwork architecture](https://github.com/meta/astryx/blob/09dafe8504dd574efe214fc463e5fc9902708a73/docs/architecture/icon-resolution-and-component-slots.md): distinguishes component-owned icon meaning from theme-selected shared artwork and acknowledges target architecture versus existing behavior. Transfer: local role overrides should not accidentally replace a shared concept everywhere.
- [Icon implementation](https://github.com/meta/astryx/blob/09dafe8504dd574efe214fc463e5fc9902708a73/packages/core/src/Icon/Icon.tsx): semantic resolution, explicit/contextual size and foreground roles. Transfer: inspect actual version and separate glyph from control behavior, without copying its size ladder or prop API.
- [Theming surface](https://github.com/meta/astryx/blob/09dafe8504dd574efe214fc463e5fc9902708a73/docs/architecture/component-theming-surface.md): component anatomy is not a one-to-one public styling API. Transfer: document supported customization and ownership, not every implementation wrapper.
- [Test sufficiency](https://github.com/meta/astryx/blob/09dafe8504dd574efe214fc463e5fc9902708a73/docs/architecture/component-test-sufficiency.md): observable promises and risks determine evidence layers, not an exhaustive prop cross-product. Transfer: actual-component, behavior, optical and accessibility evidence remain distinct.
- [Iconography draft](https://github.com/meta/astryx/blob/09dafe8504dd574efe214fc463e5fc9902708a73/docs/design/iconography.md): useful role and optical questions, explicitly draft. Its outline default and numerical size scale are not universal rules. The draft describes contextual sizing as future and omits an `info` color listed in current implementation; therefore it is not treated as an authoritative description of shipped APIs.

The pack adds its own synthesis: unlike-shape specimens, silhouette and negative-space comparison, native-size optical mass, responsive contextual review, scope-sized customization and no required library migration. These are methods to apply and test, not a measured claim that Astryx or this update improves all outputs.

## Rights and boundaries

The [upstream MIT license](https://github.com/meta/astryx/blob/09dafe8504dd574efe214fc463e5fc9902708a73/LICENSE) names Meta Platforms, Inc., copyright 2026. This update uses original prose and copies no substantial upstream text, source code or artwork. Any future substantial reuse must retain required copyright/permission notices and separately inspect asset rights. Astryx branding, default geometry, approval bureaucracy and execution/install policies are not imported.

The reusable pack remains self-contained with local method routes. External source locators are retained here outside its allowlisted archive. No runtime hook, framework dependency, private user profile or language-specific aesthetic default is added.

## Verification boundary

Package inventory/link checks and synthetic tests protect the written contract and declared outcomes. They do not run Astryx, prove host activation, test production screen readers, inspect actual icon pixels or establish aesthetic gains. The evaluation scenarios prescribe separate real-output review. Source publication is not installation or merger into the product's main branch.
