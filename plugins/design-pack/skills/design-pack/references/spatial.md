# 3D, objects and scene construction

Begin with intended use: concept image, interactive scene, animation, game asset, product visualization or manufacturing input. A plausible image is not a dimensionally verified object.

## Evidence and constraints

Record reference provenance, scale/units, camera viewpoint, visible geometry, materials, lighting and what cannot be seen. One photograph cannot establish hidden surfaces, exact dimensions, construction or material properties. Label reconstructed areas as assumptions; request additional views when hidden geometry affects the outcome.

Set a fidelity contract: silhouette and proportions that matter, views to match, required details, allowed approximation, animation needs, topology/export requirements and target performance. Do not spend most of the budget on texture while the silhouette is wrong.

## Staged production

1. **Blockout:** primary masses, axes, dimensions, contact with ground and composition. Validate silhouette from the required views.
2. **Structure:** component hierarchy, connections, functional clearances, pose and relative scale. Check intersections and unsupported assumptions.
3. **Surface:** materials, roughness, reflectance, texture scale, UVs where relevant and consistent lighting. Separate lighting effects from material color when interpreting a photo.
4. **Motion/interaction:** pivots, rigging, collision and range of movement if required. Test the actual pose/animation envelope, not just a neutral screenshot.
5. **Delivery:** target format, units, orientation, naming, polygon/texture budget, licensing and editable source. Reopen or import into the target tool when available.

Use aligned reference/render comparisons with recorded camera, lighting and scale; avoid declaring fidelity from different flattering views. Iterate on the largest discrepancy before increasing detail. When procedural code is used, preserve deterministic parameters so changes can be reviewed and reproduced.

For physical products or spaces, continue with [physical-design.md](physical-design.md). 3D visualization does not certify manufacturability, ergonomics, structural safety or regulatory compliance.
