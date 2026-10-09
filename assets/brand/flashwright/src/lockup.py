"""Flashwright horizontal app lockup: app icon (32px grid) + FLASHWRIGHT wordmark + "by Clatty Works".
Layout in icon-grid units,
IBM Plex Mono Bold caps at +40 tracking, text outlined to SVG paths with fontTools, integer PNG scales."""
import os
from PIL import Image, ImageDraw
from pixels import app_icon, PAL, MONO_KEEP, hexrgb
from build_logo import Face, F_BOLD, F_MED, CLEAR   # same font metrics + clear-space unit as the studio lockup

BOLD, MED = Face(F_BOLD), Face(F_MED)
WORD, BYLINE = "FLASHWRIGHT", "by Clatty Works"
ICON = app_icon(32)


def layout():
    size, bsize, gap = 18, 7, 8
    tw = max(BOLD.width(WORD, size), MED.width(BYLINE, bsize))
    W, H = 200, 50
    mx = round((W - (32 + gap + tw)) / 2)
    assert mx >= CLEAR
    cap, bcap = BOLD.cap * size, MED.cap * bsize
    block = cap + 5 + bcap                       # wordmark cap height + gap + byline cap height
    top = 9 + (32 - block) / 2                   # optically centre the text block on the icon
    base = top + cap
    bbase = base + 5 + bcap
    tx = mx + 32 + gap
    return dict(W=W, H=H, mark=(mx, 9), text=[(BOLD, WORD, size, tx, base, "K"), (MED, BYLINE, bsize, tx, bbase, "D")])


def rects(ox, oy, mono):
    out = []
    for y, row in enumerate(ICON.px):
        x = 0
        while x < ICON.w:
            c = row[x]
            if c is None or (mono and c not in MONO_KEEP):
                x += 1; continue
            x2 = x
            while x2 + 1 < ICON.w and row[x2 + 1] == c:
                x2 += 1
            out.append(f'<rect x="{ox+x}" y="{oy+y}" width="{x2-x+1}" height="1" fill="{"#FFFFFF" if mono else PAL[c]}"/>')
            x = x2 + 1
    return "\n".join(out)


def svg(L, path, bg=None, mono=False):
    p = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {L["W"]} {L["H"]}" width="{L["W"]*4}" height="{L["H"]*4}">',
         "<title>Flashwright by Clatty Works</title>"]
    if bg:
        p.append(f'<rect width="{L["W"]}" height="{L["H"]}" fill="{bg}"/>')
    p.append(f'<g shape-rendering="crispEdges">\n{rects(*L["mark"], mono)}\n</g>')
    for face, text, size, x, base, ink in L["text"]:
        p.append(f'<path fill="{"#FFFFFF" if mono else PAL[ink]}" d="{face.svg_path(text, size, x, base)}"/>')
    p.append("</svg>\n")
    open(path, "w").write("\n".join(p))


def png(L, path, scale, bg=None, mono=False):
    im = Image.new("RGBA", (L["W"] * scale, L["H"] * scale), bg or (0, 0, 0, 0))
    im.alpha_composite(ICON.image(scale, mono=mono), (L["mark"][0] * scale, L["mark"][1] * scale))
    d = ImageDraw.Draw(im)
    for face, text, size, x, base, ink in L["text"]:
        face.pil(d, text, size, x, base, (255, 255, 255) if mono else hexrgb(PAL[ink]), scale)
    (im.convert("RGB") if bg else im).save(path)


def build(out_dir):
    os.makedirs(out_dir, exist_ok=True)
    o = lambda n: os.path.join(out_dir, n)
    L = layout()
    svg(L, o("flashwright-lockup-horizontal.svg"))
    svg(L, o("flashwright-lockup-horizontal-on-white.svg"), bg="#FFFFFF")
    svg(L, o("flashwright-lockup-horizontal-on-deep-teal.svg"), bg=PAL["D"], mono=True)
    for sc in (4, 8):
        w = L["W"] * sc
        png(L, o(f"flashwright-lockup-horizontal-{w}.png"), sc)
        png(L, o(f"flashwright-lockup-horizontal-on-white-{w}.png"), sc, bg="#FFFFFF")
        png(L, o(f"flashwright-lockup-horizontal-on-deep-teal-{w}.png"), sc, bg=PAL["D"], mono=True)
