# Vector construction and verification

## Tools and environment

Reuse a suitable shared or project environment for art tooling rather than adding it to the application's runtime dependencies. The bundled tracing helper requires Python with Pillow and VTracer. `rsvg-convert` and ImageMagick can render and compare the artwork; `xmllint` validates XML. Use equivalent tools when those are unavailable.

The bundled `scripts/raster_to_svg.py` accepts input/output paths and uses Pillow and VTracer. It writes to a new SVG by default and requires `--force` to replace an existing output. It keeps the native canvas and adds a source-alpha-derived vector clip, because color tracing can silently fill transparent architectural openings. Thresholding is appropriate for crisp illustration plates; partial-alpha mist/glow may need authored vector gradients or masks afterward.

## Starting trace settings

The reviewed Fablemind scenes used:

```python
colormode='color'
hierarchical='cutout'
mode='spline'
filter_speckle=12
color_precision=6
layer_difference=10
corner_threshold=65
length_threshold=3
max_iterations=10
splice_threshold=45
path_precision=2
```

Treat these as starting values. Higher precision/detail may preserve tiny roots and shading at the cost of many more paths. It does not guarantee isolated detail retention: a forward test lost 5×5 and 2×2 colored islands even with permissive settings. Inspect such accents explicitly and restore important ones with authored shapes. Large flat regions and important smooth contours often benefit from manual Bézier construction. Restore a focal glow with a gradient when tracing turns it into one solid region.

For transparent sources, independently trace a black-foreground/white-background alpha mask in binary spline mode. Put its compound paths directly inside `<clipPath clipPathUnits="userSpaceOnUse">`, retaining transforms and holes, and apply that clip to the colored artwork. Direct path children avoid renderer issues seen with nested groups in clip paths.

Keep IDs and rendering order stable. Group disconnected cutout facets into useful structural parts only after checking that regrouping preserves the render. Approximate color/location grouping is editing assistance, not semantic segmentation. When modifying one part, synchronize its standalone SVG and the assembled final. Reassembly should preserve reviewed gradients and clips; restarting a raw trace can overwrite those refinements.

## Verify each asset

```sh
xmllint --noout final.svg
rsvg-convert final.svg -o rendered.png
rsvg-convert -w 128 final.svg -o small.png
magick compare -metric RMSE source.png rendered.png diff.png
```

Match canvas, crop and background before interpreting a difference score. Compare silhouette masks separately from RGB. Inspect the actual diff and both full images; a low whole-canvas error can hide a damaged focal leaf or a filled window. Inspect small filaments and transparent cutouts at native scale. Render a complete comparison set again after any refinement.

Verify there are no `<image>` nodes, base64/data-image payloads, or external references in final SVGs. Preserve native viewBox and transparent margins so replacing imports does not shift the site composition. Check IDs if inlining several SVGs on one page; separate `<img src>` documents naturally isolate their internal IDs.

## Deliverables

Keep source raster references, SVG source parts, final SVGs and comparison renders. A compact manifest with source/output hashes helps identify which approved raster a vector came from. Report whether shapes were authored or trace-assisted and identify remaining simplifications. Adoption requires a real-page check at desktop and narrow widths, correct MIME/source imports, and a successful production build.

For site adoption, compare actual image content as well as DOM rectangles. An external SVG may letterbox inside a deliberately stretched image frame because of its root preserveAspectRatio default. If preserving the existing nonuniform frame is intended, preserveAspectRatio="none" on that SVG honors the frame; retain CSS object-fit where containment is intended. This is a per-composition choice, not a default for every vector.
