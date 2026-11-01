#!/usr/bin/env python3
"""Produce a vector-only illustration trace with an independent alpha clip.

Requires Pillow and VTracer in the invoking Python environment. This is a
construction pass; inspect and refine the resulting artwork before adoption.
"""
from __future__ import annotations

import argparse
from copy import deepcopy
from pathlib import Path
from tempfile import TemporaryDirectory
import xml.etree.ElementTree as ET

NS = 'http://www.w3.org/2000/svg'
ET.register_namespace('', NS)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--alpha-threshold', type=int, default=150)
    parser.add_argument('--speckle', type=int, default=12)
    parser.add_argument('--color-precision', type=int, default=6)
    parser.add_argument('--layer-difference', type=int, default=10)
    parser.add_argument('--force', action='store_true')
    args = parser.parse_args()
    if not 0 <= args.alpha_threshold <= 255:
        parser.error('--alpha-threshold must be between 0 and 255')
    if args.source.resolve() == args.output.resolve():
        parser.error('Source and output must be different files')
    if args.output.exists() and not args.force:
        parser.error('Output exists; choose a new name or pass --force')
    try:
        import vtracer
        from PIL import Image
    except ImportError as exc:
        raise SystemExit('Run with a Python environment containing Pillow and VTracer') from exc

    with Image.open(args.source) as opened:
        source = opened.convert('RGBA')
    width, height = source.size
    alpha = source.getchannel('A')
    if alpha.getextrema()[1] <= args.alpha_threshold:
        parser.error('Source has no foreground above the alpha threshold')
    # Binary tracing uses black foreground on white. The alpha clip preserves holes
    # independently from the color trace, including white foreground objects.
    mask = alpha.point(lambda value: 0 if value > args.alpha_threshold else 255).convert('L')
    with TemporaryDirectory(prefix='illustrated-svg-') as scratch:
        temp = Path(scratch)
        raster = temp / 'source.png'
        mask_path = temp / 'alpha.png'
        color_path = temp / 'color.svg'
        alpha_path = temp / 'alpha.svg'
        source.save(raster)
        mask.save(mask_path)
        settings = dict(mode='spline', filter_speckle=args.speckle,
                        corner_threshold=65, length_threshold=3,
                        max_iterations=10, splice_threshold=45, path_precision=2)
        vtracer.convert_image_to_svg_py(str(raster), str(color_path),
                                       colormode='color', hierarchical='cutout',
                                       color_precision=args.color_precision,
                                       layer_difference=args.layer_difference, **settings)
        vtracer.convert_image_to_svg_py(str(mask_path), str(alpha_path),
                                       colormode='binary', **settings)
        root = ET.Element(f'{{{NS}}}svg', {'viewBox': f'0 0 {width} {height}',
                                         'width': str(width), 'height': str(height),
                                         'role': 'img'})
        ET.SubElement(root, f'{{{NS}}}title').text = args.source.stem.replace('-', ' ')
        defs = ET.SubElement(root, f'{{{NS}}}defs')
        clip = ET.SubElement(defs, f'{{{NS}}}clipPath',
                             {'id': 'source-alpha', 'clipPathUnits': 'userSpaceOnUse'})
        for element in ET.parse(alpha_path).getroot():
            if element.tag == f'{{{NS}}}path' and element.get('d', '').strip():
                copied = deepcopy(element)
                copied.set('clip-rule', 'evenodd')
                clip.append(copied)
        if not len(clip):
            raise SystemExit('Alpha trace produced no visible contours')
        group = ET.SubElement(root, f'{{{NS}}}g',
                              {'id': 'traced-artwork', 'clip-path': 'url(#source-alpha)'})
        for index, element in enumerate(ET.parse(color_path).getroot()):
            if element.tag == f'{{{NS}}}path' and element.get('d', '').strip():
                copied = deepcopy(element)
                copied.set('id', f'color-region-{index:04d}')
                group.append(copied)
        if not len(group):
            raise SystemExit('Color trace produced no visible contours')
        args.output.parent.mkdir(parents=True, exist_ok=True)
        ET.indent(root)
        ET.ElementTree(root).write(args.output, encoding='unicode', xml_declaration=True)
    print(f'{args.output}: {width}×{height}, {len(group)} colored regions; render and review next')


if __name__ == '__main__':
    main()
