"""Deterministic flat SVG badge and endpoint JSON generator.

Shared by both the domain health check runner and the catalog refresh script so
README badges stay crisp, legible inside Markdown tables, and consistent across
GitHub light and dark themes.
"""

from __future__ import annotations

import html
import json
import os
import tempfile
from pathlib import Path
from typing import Any

COLOR_HEX: dict[str, str] = {
    "brightgreen": "#1f883d",
    "green": "#1f883d",
    "yellow": "#9a6700",
    "orange": "#bc4c00",
    "red": "#cf222e",
    "blue": "#0969da",
    "lightgrey": "#57606a",
    "grey": "#57606a",
}

BADGE_HEIGHT = 24
PILL_HEIGHT = 26
LABEL_BG = "#24292f"
FONT_FAMILY = "-apple-system,BlinkMacSystemFont,Segoe UI,Helvetica,Arial,sans-serif"


def _text_width(text: str, *, font_size: int = 12, bold: bool = False) -> int:
    """Estimate UI sans-serif width with generous horizontal padding (no squeezing)."""
    size_scale = font_size / 12.0
    weight_scale = 1.06 if bold else 1.0
    width = 0.0
    for ch in text:
        if ch in "ilI1.,:;|!'` ·":
            width += 4.5 * size_scale * weight_scale
        elif ch in "mwMW@":
            width += 10.4 * size_scale * weight_scale
        elif ch.isupper():
            width += 8.4 * size_scale * weight_scale
        else:
            width += 7.2 * size_scale * weight_scale
    pad = 22 if font_size >= 13 else 20
    return max(56 if font_size >= 13 else 38, int(round(width)) + pad)


def render_svg(
    label: str,
    message: str,
    color: str = "lightgrey",
    *,
    aria_label: str | None = None,
) -> str:
    """Render a self-contained high-legibility flat SVG badge with viewBox scaling.

    When `label` is empty, renders a compact single-segment status pill sized for
    Markdown tables so GitHub never shrinks the text below readable size.
    """
    fill = COLOR_HEX.get(color, color if color.startswith("#") else COLOR_HEX["lightgrey"])
    esc_message = html.escape(message, quote=True)
    computed_aria = aria_label or (f"{label}: {message}" if label else message)
    aria = html.escape(computed_aria, quote=True)

    if not label:
        total = _text_width(message, font_size=13, bold=True)
        h = PILL_HEIGHT
        return (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="{h}" '
            f'viewBox="0 0 {total} {h}" role="img" aria-label="{aria}">'
            f"<title>{aria}</title>"
            f'<rect width="{total}" height="{h}" rx="6" fill="{fill}"/>'
            f'<text x="{total / 2.0:.1f}" y="{h / 2.0:.1f}" fill="#ffffff" '
            f'text-anchor="middle" dominant-baseline="central" '
            f'font-family="{FONT_FAMILY}" font-size="13" font-weight="600" '
            f'text-rendering="geometricPrecision">{esc_message}</text>'
            f"</svg>\n"
        )

    lw = _text_width(label, font_size=12, bold=False)
    mw = _text_width(message, font_size=12, bold=True)
    total = lw + mw
    h = BADGE_HEIGHT
    esc_label = html.escape(label, quote=True)
    lx = lw / 2.0
    mx = lw + mw / 2.0
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="{h}" '
        f'viewBox="0 0 {total} {h}" role="img" aria-label="{aria}">'
        f"<title>{aria}</title>"
        f'<clipPath id="r"><rect width="{total}" height="{h}" rx="4" fill="#fff"/></clipPath>'
        f'<g clip-path="url(#r)">'
        f'<rect width="{lw}" height="{h}" fill="{LABEL_BG}"/>'
        f'<rect x="{lw}" width="{mw}" height="{h}" fill="{fill}"/>'
        f"</g>"
        f'<g fill="#ffffff" text-anchor="middle" dominant-baseline="central" '
        f'font-family="{FONT_FAMILY}" font-size="12" text-rendering="geometricPrecision">'
        f'<text x="{lx:.1f}" y="{h / 2.0:.1f}" font-weight="500">{esc_label}</text>'
        f'<text x="{mx:.1f}" y="{h / 2.0:.1f}" font-weight="600">{esc_message}</text>'
        f"</g></svg>\n"
    )


def shields_endpoint_payload(
    label: str,
    message: str,
    color: str = "lightgrey",
    *,
    is_error: bool = False,
) -> dict[str, Any]:
    """Build a Shields.io endpoint JSON payload."""
    return {
        "schemaVersion": 1,
        "label": label,
        "message": message,
        "color": color,
        "isError": is_error,
    }


def write_atomic_text(path: Path, content: str) -> None:
    """Atomically write UTF-8 text to disk, creating parent directories if needed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(content)
        os.chmod(tmp_path, 0o644)
        tmp_path.replace(path)
    finally:
        tmp_path.unlink(missing_ok=True)


def write_badge(
    directory: Path,
    slug: str,
    label: str,
    message: str,
    color: str = "lightgrey",
    *,
    is_error: bool = False,
    aria_label: str | None = None,
) -> dict[str, Any]:
    """Write `<slug>.svg` and return its Shields endpoint descriptor."""
    svg_path = directory / f"{slug}.svg"
    write_atomic_text(svg_path, render_svg(label, message, color, aria_label=aria_label))
    return shields_endpoint_payload(label or slug, message, color, is_error=is_error)


def write_atomic_json(path: Path, payload: Any) -> None:
    """Atomically write pretty-formatted UTF-8 JSON."""
    write_atomic_text(path, json.dumps(payload, indent=2, sort_keys=False) + "\n")
