#!/usr/bin/env python3
"""Write the BashCutPro brand SVGs (app icon, mark, wordmarks) into bashcut/brand/.

The mark is BashCut's (a shell prompt above a timeline with a playhead), transcribed from
BashCut's `BashCut/Views/BashCutLogo.swift` (same 512-unit geometry and colours). BashCutPro adds a
"PRO" badge to the app icon so the two apps differ in the Dock. Text is Inter Bold
(assets/fonts/Inter-Bold.ttf, OFL-1.1), converted to outlines so the SVGs need no font.

Needs: fontTools (pip install fonttools). Then run bashcut/brand/render.sh for the PNG, .icns and
.ico files. Usage: python3 bashcut/brand/generate.py
"""

import argparse
import os

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
FONT = TTFont(os.path.join(ROOT, "assets/fonts/Inter-Bold.ttf"))

CYAN = "#3dccff"
BLUE = "#296bff"
VIOLET = "#7a42f0"
CURSOR_END = "#34a0ff"  # cyan blended 45 % towards blue
CLIP_COLORS = [BLUE, "#445efa", "#5f50f5", VIOLET]  # blue to violet in four steps
PLAYHEAD = "#bff2ff"
CLIPS = [(48, 316, 84, 70), (140, 316, 106, 70), (266, 316, 96, 70), (370, 316, 94, 70)]
WAVEFORMS = [
    [0.15, 0.3, 0.5, 0.35, 0.6, 0.4, 0.25, 0.45, 0.2],
    [0.2, 0.45, 0.3, 0.65, 0.4, 0.55, 0.25, 0.5, 0.3, 0.15],
    [0.25, 0.5, 0.35, 0.6, 0.3, 0.45, 0.2, 0.4],
    [0.15, 0.4, 0.55, 0.3, 0.6, 0.35, 0.45, 0.2],
]


def text_path(text, x, y, size):
    """Outline `text` in Inter Bold with its baseline at (x, y), `size` units per em.
    Returns (path data, advance width)."""
    cmap = FONT.getBestCmap()
    glyphs = FONT.getGlyphSet()
    scale = size / FONT["head"].unitsPerEm
    pen = SVGPathPen(glyphs)
    cursor = 0.0
    for ch in text:
        name = cmap[ord(ch)]
        tp = TransformPen(pen, (scale, 0, 0, -scale, x + cursor, y))
        glyphs[name].draw(tp)
        cursor += glyphs[name].width * scale
    return pen.getCommands(), cursor


def cap_height(size):
    return FONT["OS/2"].sCapHeight * size / FONT["head"].unitsPerEm


def mark(prefix, badge):
    """The 512-unit tile as SVG elements (defs + drawing). `prefix` keeps ids unique."""
    p = prefix
    defs = f"""
  <linearGradient id="{p}tile" x1="0" y1="0" x2="0" y2="512" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#121a2b"/><stop offset="1" stop-color="#05080f"/>
  </linearGradient>
  <linearGradient id="{p}prompt" x1="160" y1="110" x2="250" y2="260" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="{CYAN}"/><stop offset="1" stop-color="{BLUE}"/>
  </linearGradient>
  <linearGradient id="{p}cursor" x1="268" y1="224" x2="360" y2="264" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="{CYAN}"/><stop offset="1" stop-color="{CURSOR_END}"/>
  </linearGradient>
  <linearGradient id="{p}badge" x1="318" y1="0" x2="466" y2="0" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="{BLUE}"/><stop offset="1" stop-color="{VIOLET}"/>
  </linearGradient>
  <filter id="{p}glow" x="-50%" y="-50%" width="200%" height="200%">
    <feGaussianBlur in="SourceAlpha" stdDeviation="9"/>
    <feFlood flood-color="{CYAN}" flood-opacity="0.55"/><feComposite operator="in" in2="SourceAlpha"/>
    <feGaussianBlur stdDeviation="9"/>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <filter id="{p}headglow" x="-200%" y="-50%" width="500%" height="200%">
    <feFlood flood-color="{CYAN}" flood-opacity="0.8"/><feComposite operator="in" in2="SourceAlpha"/>
    <feGaussianBlur stdDeviation="5"/>
    <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
  </filter>
  <clipPath id="{p}clip"><rect width="512" height="512" rx="114"/></clipPath>"""
    for i, ((x, y, w, h), c) in enumerate(zip(CLIPS, CLIP_COLORS)):
        defs += f"""
  <linearGradient id="{p}c{i}" x1="0" y1="{y}" x2="0" y2="{y + h}" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="{c}" stop-opacity="0.95"/><stop offset="1" stop-color="{c}" stop-opacity="0.6"/>
  </linearGradient>"""
    body = f"""
  <rect width="512" height="512" rx="114" fill="url(#{p}tile)"/>
  <g clip-path="url(#{p}clip)">
    <g filter="url(#{p}glow)">
      <path d="M168 112 L246 182 L168 252" fill="none" stroke="url(#{p}prompt)" stroke-width="44" stroke-linecap="round" stroke-linejoin="round"/>
      <rect x="268" y="224" width="92" height="40" rx="14" fill="url(#{p}cursor)"/>
    </g>
    <rect x="38" y="304" width="436" height="94" rx="22" fill="#ffffff" fill-opacity="0.05" stroke="{BLUE}" stroke-opacity="0.35" stroke-width="2"/>"""
    for i, ((x, y, w, h), c) in enumerate(zip(CLIPS, CLIP_COLORS)):
        body += f"""
    <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="9" fill="url(#{p}c{i})" stroke="{c}" stroke-width="2"/>"""
        bars = WAVEFORMS[i]
        step = (w - 24) / (len(bars) - 1)
        for b, frac in enumerate(bars):
            bh = h * frac
            bx = x + 12 + b * step - 2
            by = y + h / 2 - bh / 2
            body += f'<rect x="{bx:.2f}" y="{by:.2f}" width="4" height="{bh:.2f}" rx="2" fill="#ffffff" fill-opacity="0.35"/>'
    body += f"""
    <g filter="url(#{p}headglow)" fill="{PLAYHEAD}">
      <rect x="253" y="290" width="6" height="118" rx="3"/>
      <path d="M240 280 L272 280 L256 300 Z"/>
    </g>"""
    if badge:
        size = 34
        d, adv = text_path("PRO", 0, 0, size)
        bx, by, bw, bh = 318, 66, 148, 60
        tx = bx + (bw - adv) / 2
        ty = by + bh / 2 + cap_height(size) / 2
        d, _ = text_path("PRO", tx, ty, size)
        body += f"""
    <rect x="{bx}" y="{by}" width="{bw}" height="{bh}" rx="30" fill="url(#{p}badge)"/>
    <path d="{d}" fill="#ffffff"/>"""
    body += "\n  </g>"
    return defs, body


def write(name, svg):
    with open(os.path.join(HERE, name), "w", encoding="utf-8") as f:
        f.write(svg)
    print("wrote", os.path.relpath(os.path.join(HERE, name), ROOT))


def tile_svg(badge, title):
    defs, body = mark("", badge)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">\n'
        f"<title>{title}</title>\n<defs>{defs}\n</defs>{body}\n</svg>\n"
    )


def wordmark_svg(dark_ink):
    """The mark (no badge) and "BashCutPro", 512 units tall like the tile. `dark_ink` is for light
    backgrounds."""
    defs, body = mark("m", False)
    size = 400
    gap = 64
    base = 256 + cap_height(size) / 2
    d1, a1 = text_path("BashCut", 512 + gap, base, size)
    d2, a2 = text_path("Pro", 512 + gap + a1 + 8, base, size)
    width = 512 + gap + a1 + 8 + a2 + 16
    ink = "#0b1220" if dark_ink else "#f5f7fa"
    pro = (BLUE, VIOLET) if dark_ink else (CYAN, VIOLET)
    x0 = 512 + gap + a1
    defs += f"""
  <linearGradient id="pro" x1="{x0:.0f}" y1="0" x2="{x0 + a2:.0f}" y2="0" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="{pro[0]}"/><stop offset="1" stop-color="{pro[1]}"/>
  </linearGradient>"""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width:.0f} 512" width="{width:.0f}" height="512">\n'
        f"<title>BashCutPro</title>\n<defs>{defs}\n</defs>\n<g>{body}</g>\n"
        f'<path d="{d1}" fill="{ink}"/>\n<path d="{d2}" fill="url(#pro)"/>\n</svg>\n'
    )


def main(argv=None):
    argparse.ArgumentParser(description=__doc__.splitlines()[0]).parse_args(argv)
    os.makedirs(os.path.join(HERE, "app-icon"), exist_ok=True)
    os.makedirs(os.path.join(HERE, "wordmark"), exist_ok=True)
    write("app-icon/bashcutpro.svg", tile_svg(True, "BashCutPro"))
    write("wordmark/bashcutpro-wordmark.svg", wordmark_svg(True))
    write("wordmark/bashcutpro-wordmark-white.svg", wordmark_svg(False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
