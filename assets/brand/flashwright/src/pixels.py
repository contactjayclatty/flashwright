"""Flashwright pixel art: toolbar/step icons, wizard banner, app icon.
Hand-placed pixel shapes on integer grids, same construction rules as the Clatty Works mark
Charcoal 1px outline, Panel Grey bodies with 1px bevels, solid Deep Teal
title strips, Wizard Teal / Signal Amber fills. No anti-aliasing, no third-party glyphs."""
import os, sys
from PIL import Image, ImageDraw

LOGO_SRC = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "logo", "src"))
sys.path.insert(0, LOGO_SRC)
from draw import PAL, MONO_KEEP, hexrgb  # noqa: E402  (single source of palette truth)


class G:
    """Rectangular pixel grid (the logo's Grid is square-only)."""
    def __init__(s, w, h=None):
        s.w, s.h = w, h or w
        s.px = [[None] * s.w for _ in range(s.h)]

    def set(s, x, y, c):
        if 0 <= x < s.w and 0 <= y < s.h:
            s.px[y][x] = c

    def get(s, x, y):
        return s.px[y][x] if 0 <= x < s.w and 0 <= y < s.h else None

    def rect(s, x, y, w, h, c):
        for j in range(y, y + h):
            for i in range(x, x + w):
                s.set(i, j, c)

    def fill(s, pts, c, outline="K"):
        """Fill a pixel set and wrap it in a 1px 4-connected outline ring."""
        pts = set(pts)
        if outline:
            for (x, y) in pts:
                for p in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                    if p not in pts:
                        s.set(*p, outline)
        for p in pts:
            s.set(*p, c)

    def art(s, rows, ox=0, oy=0):
        for j, row in enumerate(rows):
            for i, ch in enumerate(row):
                if ch not in ". ":
                    s.set(ox + i, oy + j, ch)

    def pane(s, x, y, w, h, strip=2, screen=None, bevel=True):
        """Bevelled pane: outline, solid Deep Teal strip, grey body, light TL / dark BR bevel."""
        s.rect(x, y, w, h, "K")
        s.rect(x + 1, y + 1, w - 2, strip, "D")
        by = y + 1 + strip
        s.rect(x + 1, by, w - 2, y + h - 1 - by, "G")
        if bevel:
            s.rect(x + 1, by, w - 2, 1, "L"); s.rect(x + 1, by, 1, y + h - 1 - by, "L")
            s.rect(x + 1, y + h - 2, w - 2, 1, "S"); s.rect(x + w - 2, by + 1, 1, y + h - 2 - by, "S")
        else:
            s.rect(x + 1, y + h - 2, w - 2, 1, "S")
        if screen:
            ix, iy, iw, ih = screen
            s.rect(x + ix, y + iy, iw, ih, "W")

    # ---- output ----
    def image(s, scale=1, bg=None, mono=False):
        """mono=True: all-white single-colour version (structure pixels K/T/D white, bodies transparent),
        same rule as the logo's MONO_KEEP."""
        im = Image.new("RGBA", (s.w, s.h), (0, 0, 0, 0) if bg is None else hexrgb(bg) + (255,))
        for y, row in enumerate(s.px):
            for x, c in enumerate(row):
                if c is None or (mono and c not in MONO_KEEP):
                    continue
                im.putpixel((x, y), (255, 255, 255, 255) if mono else hexrgb(PAL[c]) + (255,))
        if scale != 1:
            im = im.resize((s.w * scale, s.h * scale), Image.NEAREST)
        return im

    def svg(s, path, title="", scale=1):
        rects = []
        for y, row in enumerate(s.px):
            x = 0
            while x < s.w:
                c = row[x]
                if c is None:
                    x += 1; continue
                x2 = x
                while x2 + 1 < s.w and row[x2 + 1] == c:
                    x2 += 1
                rects.append(f'<rect x="{x}" y="{y}" width="{x2-x+1}" height="1" fill="{PAL[c]}"/>')
                x = x2 + 1
        with open(path, "w") as f:
            f.write(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {s.w} {s.h}" '
                    f'width="{s.w*scale}" height="{s.h*scale}" shape-rendering="crispEdges">\n')
            if title:
                f.write(f"<title>{title}</title>\n")
            f.write("\n".join(rects) + "\n</svg>\n")


# ---------------- shape helpers (return pixel sets) ----------------
def R(x, y, w, h):
    return {(i, j) for i in range(x, x + w) for j in range(y, y + h)}


def poly(pts, size=64):
    im = Image.new("1", (size, size), 0)
    ImageDraw.Draw(im).polygon(pts, fill=1, outline=1)
    return {(x, y) for x in range(size) for y in range(size) if im.getpixel((x, y))}


def disk(cx, cy, r):
    """Disk centred on continuous point (cx, cy); pixel centres at +0.5."""
    return {(x, y) for x in range(int(cx - r) - 1, int(cx + r) + 2) for y in range(int(cy - r) - 1, int(cy + r) + 2)
            if (x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2 <= r * r}


def arrow_pts(x0, cy, shaft, t, hh, d=1, vertical=False):
    """Chunky 45-degree pixel arrow (same geometry as the logo arrow). d=1 right/down, -1 left/up."""
    pts = set()
    for k in range(shaft):
        for o in range(-(t // 2), t // 2 + 1):
            pts.add((x0 + d * k, cy + o))
    for k in range(hh + 1):
        for o in range(-(hh - k), hh - k + 1):
            pts.add((x0 + d * (shaft + k), cy + o))
    return {(b, a) for (a, b) in pts} if vertical else pts


BOLT32 = [  # amber lightning bolt, outlined (14 x 24)
    "......KKKKKK..",
    ".....KAAAAAK..",
    ".....KAAAAK...",
    "....KAAAAAK...",
    "....KAAAAK....",
    "...KAAAAAK....",
    "...KAAAAK.....",
    "..KAAAAAKKKKK.",
    "..KAAAAAAAAAK.",
    ".KAAAAAAAAAK..",
    ".KKKKKAAAAK...",
    ".....KAAAK....",
    "....KAAAK.....",
    "....KAAK......",
    "...KAAK.......",
    "...KAK........",
    "..KAK.........",
    "..KK..........",
]
BOLT16 = [  # 8 x 13
    "...KKKK",
    "..KAAAK",
    "..KAAK.",
    ".KAAAK.",
    ".KAAKKK",
    "KAAAAAK",
    "KKKAAK.",
    "..KAAK.",
    "..KAK..",
    ".KAK...",
    ".KK....",
]


# =====================================================================
#                              ICONS
# =====================================================================
def phone(g, x, y, w, h, small=False):
    """Generic slab phone: outline, Deep Teal top strip, white screen, grey chin with square dot.
    Deliberately generic: no camera bar, no rounded corners, nothing model-specific."""
    if small:
        g.rect(x, y, w, h, "K"); g.rect(x + 1, y + 1, w - 2, 1, "D")
        g.rect(x + 1, y + 2, w - 2, h - 5, "W"); g.rect(x + 1, y + h - 3, w - 2, 1, "G")
        g.rect(x + 1, y + h - 2, w - 2, 1, "S")
    else:
        g.pane(x, y, w, h, strip=2, screen=(2, 4, w - 4, h - 10))
        g.rect(x + w // 2 - 1, y + h - 4, 2 if w % 2 == 0 else 1, 1, "K")


def icon_phone_connect(n):
    g = G(n)
    if n == 32:
        phone(g, 1, 1, 13, 30)
        g.rect(15, 11, 3, 2, "K"); g.rect(15, 19, 3, 2, "K")          # prongs
        g.pane(18, 8, 8, 16, strip=0)                                  # plug body
        g.rect(20, 11, 4, 2, "S"); g.rect(20, 19, 4, 2, "S")           # grip lines
        g.fill(R(26, 14, 6, 4), "T")                                   # cable
        g.rect(26, 15, 1, 2, "T")
    else:
        phone(g, 0, 0, 7, 16, small=True)
        g.rect(3, 13, 1, 1, "K")
        g.rect(8, 5, 2, 1, "K"); g.rect(8, 9, 2, 1, "K")               # prongs
        g.rect(10, 3, 4, 9, "K"); g.rect(11, 4, 2, 7, "G"); g.rect(11, 4, 1, 7, "L")
        g.rect(12, 10, 1, 1, "S")
        g.rect(14, 6, 2, 3, "T"); g.rect(14, 5, 2, 1, "K"); g.rect(14, 9, 2, 1, "K")
    return g


def tray(g, n):
    if n == 32:
        g.rect(1, 19, 30, 12, "K")
        g.rect(2, 20, 4, 6, "G"); g.rect(26, 20, 4, 6, "G")
        g.rect(2, 20, 1, 6, "L"); g.rect(26, 20, 1, 6, "L")
        g.rect(6, 20, 20, 5, None)
        g.rect(6, 24, 20, 1, "K")
        g.rect(2, 26, 28, 4, "G"); g.rect(2, 26, 28, 1, "L"); g.rect(2, 29, 28, 1, "S")
        g.rect(24, 27, 3, 1, "A")
        for x in (6, 25):
            g.rect(x, 20, 1, 4, "K")
    else:
        g.rect(0, 10, 16, 6, "K")
        g.rect(1, 10, 2, 3, "G"); g.rect(13, 10, 2, 3, "G")
        g.rect(3, 10, 10, 2, None); g.rect(3, 12, 10, 1, "K")
        g.rect(1, 13, 14, 2, "G"); g.rect(1, 14, 14, 1, "S"); g.rect(11, 13, 2, 1, "A")
        g.rect(0, 10, 1, 3, "K"); g.rect(15, 10, 1, 3, "K"); g.rect(3, 10, 1, 2, "K"); g.rect(12, 10, 1, 2, "K")


def icon_download(n):
    g = G(n)
    tray(g, n)
    if n == 32:
        g.fill(arrow_pts(1, 16, 12, 7, 7, 1, vertical=True), "T")
    else:
        g.fill(arrow_pts(1, 7, 5, 3, 4, 1, vertical=True), "T")
        g.rect(0, 10, 1, 1, "K")
    return g


def icon_root(n):
    """Key: amber bow with a square hole, teal shaft and teeth (our drawing, not a symbol font)."""
    g = G(n)
    if n == 32:
        bow = disk(9, 16, 7.6) - R(7, 14, 4, 4)
        shaft = R(16, 14, 14, 4) | R(22, 18, 3, 5) | R(27, 18, 3, 4)
        g.fill(shaft, "T")
        g.fill(bow, "A")
        g.rect(7, 14, 4, 4, None)
        for (x, y) in R(6, 13, 6, 6) - R(7, 14, 4, 4):
            g.set(x, y, "K")
        g.rect(17, 15, 12, 1, "D")  # groove
    else:
        bow = disk(4.5, 8, 4.2) - R(4, 7, 2, 2)
        shaft = R(9, 7, 6, 2) | R(11, 9, 1, 2) | R(13, 9, 2, 2)
        g.fill(shaft, "T")
        g.fill(bow, "A")
        g.rect(4, 7, 2, 2, None)
        for (x, y) in R(3, 6, 4, 4) - R(4, 7, 2, 2):
            g.set(x, y, "K")
    return g


def icon_flash(n):
    """Chip with an amber bolt."""
    g = G(n)
    if n == 32:
        for k in range(4):                                   # pins
            p = 8 + k * 5
            g.rect(p, 1, 2, 4, "K"); g.rect(p, 27, 2, 4, "K")
            g.rect(1, p, 4, 2, "K"); g.rect(27, p, 4, 2, "K")
        g.pane(4, 4, 24, 24, strip=0)
        g.rect(7, 7, 18, 18, "K"); g.rect(8, 8, 16, 16, "D")
        g.art([
            "....KKKKKK",
            "...KAAAAAK",
            "...KAAAAK.",
            "..KAAAAK..",
            "..KAAAAKKK",
            ".KAAAAAAAK",
            ".KAAAAAAK.",
            "KKKKAAAK..",
            "...KAAK...",
            "..KAAK....",
            "..KAK.....",
            ".KAK......",
            ".KK.......",
        ], 11, 9)
    else:
        for p in (4, 7, 10):
            g.rect(p, 0, 2, 2, "K"); g.rect(p, 14, 2, 2, "K")
            g.rect(0, p, 2, 2, "K"); g.rect(14, p, 2, 2, "K")
        g.rect(2, 2, 12, 12, "K"); g.rect(3, 3, 10, 10, "D")
        g.art(BOLT16, 5, 3)
    return g


def icon_backup(n):
    """Stacked backup cartridges with an amber activity light and a teal arrow."""
    g = G(n)
    if n == 32:
        # three stacked copies; the newest has the amber activity light and a teal "save out" arrow
        g.pane(6, 1, 24, 9, strip=0); g.rect(9, 4, 10, 2, "K"); g.rect(24, 4, 3, 2, "S")
        g.pane(4, 8, 24, 9, strip=0); g.rect(7, 11, 10, 2, "K"); g.rect(22, 11, 3, 2, "S")
        g.pane(2, 15, 24, 9, strip=0); g.rect(5, 18, 10, 2, "K"); g.rect(20, 18, 3, 2, "A")
        g.fill(arrow_pts(23, 25, 1, 3, 4, 1, vertical=False) | R(14, 24, 10, 3), "T")
    else:
        g.rect(4, 0, 12, 6, "K"); g.rect(5, 1, 10, 4, "G"); g.rect(6, 2, 5, 1, "K"); g.rect(5, 4, 10, 1, "S")
        g.rect(2, 4, 12, 6, "K"); g.rect(3, 5, 10, 4, "G"); g.rect(4, 6, 5, 1, "K"); g.rect(3, 8, 10, 1, "S")
        g.rect(0, 8, 12, 6, "K"); g.rect(1, 9, 10, 4, "G"); g.rect(2, 10, 5, 1, "K"); g.rect(1, 12, 10, 1, "S")
        g.rect(9, 10, 1, 1, "A")
        g.fill(R(10, 14, 3, 1) | {(13, 13), (13, 14), (13, 15), (14, 14)}, "T")
    return g


def icon_settings(n):
    g = G(n)
    if n == 32:
        import math
        c = 15.99
        body = set()
        for x in range(32):
            for y in range(32):
                dx, dy = x + 0.5 - c, y + 0.5 - c
                r = math.hypot(dx, dy)
                if r <= 10.2:
                    body.add((x, y)); continue
                if r <= 14.6:
                    # 8 parallel-sided teeth, 7px wide: distance from the nearest tooth axis <= 3.5
                    for k in range(8):
                        t = k * math.pi / 4
                        along = dx * math.cos(t) + dy * math.sin(t)
                        across = -dx * math.sin(t) + dy * math.cos(t)
                        if along > 0 and abs(across) <= 3.5:
                            body.add((x, y)); break
        hole = disk(c, c, 4.4)
        g.fill(body - hole, "T")
        for p in disk(c, c, 5.6) - hole:
            g.set(*p, "K")
        for p in hole:
            g.set(*p, "W")
    else:
        g.art([
            "......KKKK......",
            "......KTTK......",
            "..KK.KKTTKK.KK..",
            "..KTKKTTTTKKTK..",
            "..KTTTTTTTTTTK..",
            "...KTTTKKTTTK...",
            "KKKKTTKWWKTTKKKK",
            "KTTTTKWWWWKTTTTK",
            "KTTTTKWWWWKTTTTK",
            "KKKKTTKWWKTTKKKK",
            "...KTTTKKTTTK...",
            "..KTTTTTTTTTTK..",
            "..KTKKTTTTKKTK..",
            "..KK.KKTTKK.KK..",
            "......KTTK......",
            "......KKKK......",
        ])
    return g


def icon_warning(n):
    g = G(n)
    if n == 32:
        tri = poly([(15.5, 2), (30, 28), (1, 28)])
        g.fill(tri, "A")
        g.rect(14, 10, 4, 10, "K"); g.rect(14, 22, 4, 3, "K")
    else:
        g.art([
            ".......KK.......",
            "......KAAK......",
            "......KAAK......",
            ".....KAAAAK.....",
            ".....KAKKAK.....",
            "....KAAKKAAK....",
            "....KAAKKAAK....",
            "...KAAAKKAAAK...",
            "...KAAAKKAAAK...",
            "..KAAAAAAAAAAK..",
            "..KAAAAKKAAAAK..",
            ".KAAAAAKKAAAAAK.",
            ".KAAAAAAAAAAAAK.",
            "KKKKKKKKKKKKKKKK",
        ], 0, 1)
    return g


def icon_success(n):
    g = G(n)
    if n == 32:
        g.fill(disk(16, 16, 14.6), "T")
        chk = set()
        for k in range(6):
            chk |= R(7 + k, 15 + k, 3, 3)
        for k in range(12):
            chk |= R(13 + k, 20 - k, 3, 3)
        chk = {p for p in chk if p[1] >= 7}
        g.fill(chk, "W")
    else:
        g.art([
            ".....KKKKKK.....",
            "...KKTTTTTTKK...",
            "..KTTTTTTTTTTK..",
            ".KTTTTTTTTTKKTK.",
            ".KTTTTTTTTKWWKK.",
            "KTTTKKTTTKWWKTTK",
            "KTTKWWKTKWWKTTTK",
            "KTTKWWWKWWKTTTTK",
            "KTTTKWWWWKTTTTTK",
            "KTTTTKWWKTTTTTTK",
            ".KTTTTKKTTTTTTK.",
            ".KTTTTTTTTTTTTK.",
            "..KTTTTTTTTTTK..",
            "...KKTTTTTTKK...",
            ".....KKKKKK.....",
        ], 0, 0)
    return g


def icon_log(n):
    """Console pane: Deep Teal strip, charcoal body, light text lines, amber prompt."""
    g = G(n)
    if n == 32:
        g.rect(1, 3, 30, 26, "K"); g.rect(2, 4, 28, 3, "D")
        g.rect(26, 5, 2, 1, "G")
        g.rect(2, 7, 28, 21, "K")
        g.rect(3, 7, 26, 1, "S")
        g.art(["A...", ".A..", "..A.", ".A..", "A..."], 4, 10)
        g.rect(9, 12, 14, 1, "L")
        g.rect(4, 17, 18, 1, "L"); g.rect(4, 20, 12, 1, "L")
        g.rect(4, 23, 3, 2, "A")
        g.rect(1, 28, 30, 1, "K")
    else:
        g.rect(0, 1, 16, 14, "K"); g.rect(1, 2, 14, 2, "D")
        g.art(["A..", ".A.", "A.."], 2, 5)
        g.rect(5, 6, 6, 1, "L"); g.rect(2, 9, 9, 1, "L"); g.rect(2, 11, 6, 1, "L")
        g.rect(9, 11, 2, 1, "A")
    return g


def icon_help(n):
    """Speech bubble with a Deep Teal question mark."""
    g = G(n)
    if n == 32:
        g.fill(R(2, 2, 28, 21) | poly([(7, 22), (14, 22), (7, 29)]), "W")
        g.art([
            "..DDDDDD..",
            ".DDDDDDDD.",
            "DDD....DDD",
            "DDD....DDD",
            "......DDD.",
            ".....DDD..",
            "....DDD...",
            "....DDD...",
            "..........",
            "....DDD...",
            "....DDD...",
        ], 11, 7)
    else:
        g.art([
            ".KKKKKKKKKKKKKK.",
            "KWWWWWWWWWWWWWWK",
            "KWWWWWDDDDWWWWWK",
            "KWWWWDDWWDDWWWWK",
            "KWWWWWWWWDDWWWWK",
            "KWWWWWWWDDWWWWWK",
            "KWWWWWWDDWWWWWWK",
            "KWWWWWWWWWWWWWWK",
            "KWWWWWWDDWWWWWWK",
            "KWWWWWWWWWWWWWWK",
            ".KKKWWKKKKKKKKK.",
            "...KWK..........",
            "...KK...........",
        ], 0, 1)
    return g


ICONS = [
    ("phone-connect", icon_phone_connect),
    ("download", icon_download),
    ("root", icon_root),
    ("flash", icon_flash),
    ("backup", icon_backup),
    ("settings", icon_settings),
    ("warning", icon_warning),
    ("success", icon_success),
    ("log", icon_log),
    ("help", icon_help),
]


# =====================================================================
#                              APP ICON
# A generic slab phone with an amber bolt on its screen, and a teal arrow driving into it from the
# left (the PC pushing firmware to the phone). Same pane/arrow construction as mark A, but the
# phone is the hero and the arrow points the other way, so it reads as family, not a copy.
# =====================================================================
BOLT_ICON32 = [
    "....KKKKKK",
    "...KAAAAAK",
    "...KAAAAK.",
    "..KAAAAK..",
    "..KAAAAKKK",
    ".KAAAAAAAK",
    ".KAAAAAAK.",
    "KKKKAAAK..",
    "...KAAK...",
    "..KAAK....",
    "..KAK.....",
    ".KAK......",
    ".KK.......",
]
BOLT_ICON24 = [
    "..KKKK",
    ".KAAAK",
    ".KAAK.",
    "KAAAKK",
    "KAAAAK",
    "KKAAK.",
    ".KAK..",
    "KAK...",
    "KK....",
]
BOLT_ICON16 = [
    "...KKKK",
    "..KAAAK",
    "..KAAK.",
    ".KAAK..",
    ".KAAAAK",
    "KKKAAK.",
    "..KAK..",
    ".KAK...",
    ".KK....",
]


def app_icon(n):
    g = G(n)
    if n == 32:
        g.pane(13, 1, 16, 30, strip=2, screen=(2, 4, 12, 20))
        g.rect(20, 26, 2, 1, "K")
        g.art(BOLT_ICON32, 16, 8)
        g.fill(arrow_pts(1, 15, 6, 5, 5, 1), "T")
    elif n == 24:
        g.pane(10, 1, 12, 22, strip=1, screen=(2, 3, 8, 13), bevel=True)
        g.rect(15, 19, 2, 1, "K")
        g.art(BOLT_ICON24, 13, 5)
        g.fill(arrow_pts(1, 11, 3, 3, 4, 1), "T")
    else:  # 16px simplified drawing: no bevel highlight, thinner arrow
        g.rect(7, 0, 9, 16, "K"); g.rect(8, 1, 7, 1, "D")
        g.rect(8, 2, 7, 10, "W"); g.rect(8, 12, 7, 2, "G"); g.rect(8, 14, 7, 1, "S")
        g.rect(11, 13, 1, 1, "K")
        g.art(BOLT_ICON16, 8, 2)
        g.fill(arrow_pts(1, 7, 1, 3, 3, 1), "T")
    return g


# =====================================================================
#                         WIZARD SIDE BANNER
# 82 x 157 art grid, rendered at 2x (164 x 314) and 4x (328 x 628 = @2x).
# =====================================================================
def banner():
    W, H = 82, 157
    g = G(W, H)
    g.rect(0, 0, W, H, "D")
    for y in range(3, H, 8):                      # sparse teal dot grid: a quiet "desktop" texture
        for x in range(3 + (4 if (y // 8) % 2 else 0), W, 8):
            g.set(x, y, "T")
    # desktop pane
    dx, dy, dw, dh = 6, 12, 70, 46
    g.rect(dx + 2, dy + 2, dw, dh, "K")           # hard drop edge, 1 grid px (no blur, no gradient)
    g.pane(dx, dy, dw, dh, strip=4)
    for k in range(3):                            # our own plain square glyphs
        g.rect(dx + dw - 6 - 4 * k, dy + 2, 2, 2, "G")
    g.rect(dx + 4, dy + 9, 20, 2, "K")            # "heading" line
    g.rect(dx + 4, dy + 14, 44, 1, "S"); g.rect(dx + 4, dy + 17, 36, 1, "S")
    # segmented progress meter in the desktop pane
    px, py = dx + 4, dy + 24
    g.rect(px, py, dw - 8, 8, "K")
    g.rect(px + 1, py + 1, dw - 10, 6, "W")
    for i in range(6):
        g.rect(px + 2 + i * 7, py + 2, 6, 4, "T")
    # three tiny bevelled buttons
    for i, c in enumerate("GGT"):
        bx = dx + dw - 4 - 14 - i * 16
        g.rect(bx, dy + 36, 14, 6, "K"); g.rect(bx + 1, dy + 37, 12, 4, c)
        if c == "G":
            g.rect(bx + 1, dy + 37, 12, 1, "L"); g.rect(bx + 1, dy + 40, 12, 1, "S")
    # arrow down
    g.fill(arrow_pts(63, 41, 9, 7, 8, 1, vertical=True), "T")
    # phone
    ph_x, ph_y, ph_w, ph_h = 24, 86, 34, 62
    g.rect(ph_x + 2, ph_y + 2, ph_w, ph_h, "K")
    g.pane(ph_x, ph_y, ph_w, ph_h, strip=3, screen=(3, 6, ph_w - 6, ph_h - 16))
    g.rect(ph_x + ph_w // 2 - 2, ph_y + ph_h - 6, 4, 2, "K")
    # bolt on the phone screen (big)
    big = [
        ".......KKKKKKKK",
        "......KAAAAAAAK",
        "......KAAAAAAK.",
        ".....KAAAAAAK..",
        ".....KAAAAAK...",
        "....KAAAAAAK...",
        "....KAAAAAK....",
        "...KAAAAAAKKKKK",
        "...KAAAAAAAAAAK",
        "..KAAAAAAAAAAK.",
        "..KAAAAAAAAAK..",
        ".KKKKKKAAAAK...",
        "......KAAAK....",
        ".....KAAAK.....",
        ".....KAAK......",
        "....KAAK.......",
        "....KAK........",
        "...KAK.........",
        "...KK..........",
    ]
    g.art(big, ph_x + (ph_w - 15) // 2, ph_y + 13)
    # mini progress meter under the bolt on the phone, amber "in progress" cell
    mx, my = ph_x + 6, ph_y + 36
    g.rect(mx, my, ph_w - 12, 5, "K"); g.rect(mx + 1, my + 1, ph_w - 14, 3, "W")
    for i in range(4):
        g.rect(mx + 2 + i * 5, my + 2, 4, 1, "T")
    return g
