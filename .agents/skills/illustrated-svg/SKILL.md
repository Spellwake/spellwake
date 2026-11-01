---
name: illustrated-svg
description: Generate cohesive illustrations and reconstruct them as editable SVG artwork. Use when creating illustrated SVG assets, converting generated artwork into vectors, or extracting a matching illustration set; use ordinary vector editing for existing SVGs.
---

# Illustrated SVG artwork

Create an approved visual world first, then preserve it while changing medium. The target is recognizable artwork expressed as editable paths, gradients and clips.

## Establish the visual contract

Read the project's design direction and inspect current artwork. Preserve the user's subject, palette, framing and level of detail. For this specific botanical/architectural aesthetic, read [references/prompts.md](references/prompts.md). Adapt its subject variables rather than adding plants or arches to unrelated briefs.

Name the invariants: canvas and transparent margins, silhouettes, perspective, major objects, palette, and interior openings. Record exact prompts and source/output paths as work proceeds. A local defect calls for a local correction, keeping liked details intact.

## Generate and extract

Use the available image-generation tool for raster generation and edits. Read its imagegen skill when available. If composition is unresolved, generate a coherent composition reference before separate assets. Once the composition is chosen, extract each scene as a transparent plate with the same placement and margins. Inspect the reference before editing it.

Simplify texture in a separate edit when needed: smooth contours, broad color planes, limited cel shading, retained depth and recognizable details. Verify the actual output; a flat-looking PNG remains a raster. Preserve its source as the comparison master.

Completion: every plate has the intended scene, readable silhouettes and genuine transparency, including openings inside architecture.

## Reconstruct as SVG

Choose by complexity:

- **Simple shapes or signature details:** author cubic Bézier paths, tapered filled veins, clipped color planes and gradients. Keep physical parts separable where useful.
- **Dense scenes whose geometry is already approved:** use tracing as construction assistance, then inspect and refine the curves, focal gradients and contour defects. Read [references/vector-workflow.md](references/vector-workflow.md) for tools, starting settings and comparison checks. `scripts/raster_to_svg.py` provides a portable first pass with a vector alpha clip. It does not perform semantic object segmentation or replace visual refinement.

For a mixed set, combine both methods. Distinguish hand-authored from trace-assisted work when reporting it. A finished SVG contains actual vector geometry, not a base64 image wrapper.

Preserve native viewBox and source aspect ratio. Separate editable features into named groups and standalone parts as warranted; color/location groups alone are not fully rigged animation objects. Keep an external-file image reference for ordinary site display; use inline SVG only when internal animation or styling requires it.

When the intended use includes articulated animation, plan object structure before reconstruction: complete objects, connected parts, attachment points, and geometry that can deform together. A color trace does not identify individual trees, their branches, or water surfaces. Reconstruct those semantic parts and verify coordinated motion before calling an asset animation-ready. Moving an added highlight is an accent effect, not animation of the underlying water.

## Compare and repair

Validate XML, render at native dimensions and a small display size, and compare against the source on the same ground. Inspect source, render and difference image. Numerical similarity is a guide; inspect all transparent holes and the subject's important details independently.

Repair the largest material mismatch first: silhouettes, missing openings, displaced parts, lost focal lighting, or broken vein junctions. Preserve a smooth leaf boundary when that is the approved shape; branching veins remain inside a clip. Restore soft glow with a vector gradient rather than a flat brown patch. Synchronize final SVG and its standalone source parts after each repair, then re-render.

Completion: the full set is XML-valid, renders correctly, preserves important geometry/transparency, and contains no raster image nodes or external asset references. Note any meaningful loss of shading or tiny filaments rather than presenting the conversion as pixel-identical.

## Deliver or adopt

Provide the SVGs plus a source/render comparison. Keep raster masters as intentional production references. When the user authorizes site adoption, replace existing asset imports without changing the layout; verify the real page at desktop and phone widths and run the relevant build/checks. Conversion alone is not authorization to deploy or publish.

Save the project's actual prompt history and adopted source paths in its documentation. Keep this illustration workflow separate from any general-purpose image-to-svg skill installed in the environment.
