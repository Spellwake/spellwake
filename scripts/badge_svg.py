"""Deterministic Shields-style flat SVG badge and endpoint JSON generator.

Shared by both the domain health check runner and the catalog refresh script so
README badges render identically in private GitHub repositories and offline.
"""

from __future__ import annotations

import html
import json
import os
import tempfile
from pathlib import Path
from typing import Any

COLOR_HEX: dict[str, str] = {
    "brightgreen": "#4c1",
    "green": "#2ea043",
    "yellow": "#d29922",
    "orange": "#db6d28",
    "red": "#cf222e",
    "blue": "#0969da",
    "lightgrey": "#6e7781",
    "grey": "#6e7781",
}


def _text_width(text: str) -> int:
    """Estimate Verdana 11px width in pixels (conservative, deterministic, ASCII-safe)."""
    width = 0.0
    for ch in text:
        if ch in "ilI1.,:;|!'` ":
            width += 3.8
        elif ch in "mwMW@":
            width += 9.2
        elif ch.isupper():
            width += 7.3
        else:
            width += 6.4
    return max(24, int(round(width)) + 12)


def render_svg(label: str, message: str, color: str = "lightgrey") -> str:
    """Render a self-contained flat SVG badge matching Shields.io proportions."""
    fill = COLOR_HEX.get(color, color if color.startswith("#") else COLOR_HEX["lightgrey"])
    lw = _text_width(label)
    mw = _text_width(message)
    total = lw + mw
    esc_label = html.escape(label, quote=True)
    esc_message = html.escape(message, quote=True)
    aria = html.escape(f"{label}: {message}", quote=True)
    lx = lw * 5
    mx = lw * 10 + mw * 5
    l_len = max(10, (lw - 10) * 10)
    m_len = max(10, (mw - 10) * 10)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{total}" height="20" '
        f'role="img" aria-label="{aria}">'
        f"<title>{aria}</title>"
        f'<linearGradient id="s" x2="0" y2="100%">'
        f'<stop offset="0" stop-color="#bbb" stop-opacity=".1"/>'
        f'<stop offset="1" stop-opacity=".1"/>'
        f"</linearGradient>"
        f'<clipPath id="r"><rect width="{total}" height="20" rx="3" fill="#fff"/></clipPath>'
        f'<g clip-path="url(#r)">'
        f'<rect width="{lw}" height="20" fill="#555"/>'
        f'<rect x="{lw}" width="{mw}" height="20" fill="{fill}"/>'
        f'<rect width="{total}" height="20" fill="url(#s)"/>'
        f"</g>"
        f'<g fill="#fff" text-anchor="middle" '
        f'font-family="Verdana,Geneva,DejaVu Sans,sans-serif" '
        f'text-rendering="geometricPrecision" font-size="110">'
        f'<text aria-hidden="true" x="{lx}" y="150" fill="#010101" fill-opacity=".3" '
        f'transform="scale(.1)" textLength="{l_len}">{esc_label}</text>'
        f'<text x="{lx}" y="140" transform="scale(.1)" fill="#fff" '
        f'textLength="{l_len}">{esc_label}</text>'
        f'<text aria-hidden="true" x="{mx}" y="150" fill="#010101" fill-opacity=".3" '
        f'transform="scale(.1)" textLength="{m_len}">{esc_message}</text>'
        f'<text x="{mx}" y="140" transform="scale(.1)" fill="#fff" '
        f'textLength="{m_len}">{esc_message}</text>'
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
) -> dict[str, Any]:
    """Write `<slug>.svg` and return its Shields endpoint descriptor."""
    svg_path = directory / f"{slug}.svg"
    write_atomic_text(svg_path, render_svg(label, message, color))
    return shields_endpoint_payload(label, message, color, is_error=is_error)


def write_atomic_json(path: Path, payload: Any) -> None:
    """Atomically write pretty-formatted UTF-8 JSON."""
    write_atomic_text(path, json.dumps(payload, indent=2, sort_keys=False) + "\n")
