"""Build the Flashwright brand assets.
Run:  python3 build.py            (art only: icons, banner, app icon, preview sheet)
      python3 build.py --shots    (also renders the demo.html screenshots with headless Chrome)
Needs Pillow; screenshots need google-chrome or chromium on PATH."""
import os, sys, shutil, subprocess
from PIL import Image, ImageDraw, ImageFont
import pixels
import lockup
from pixels import ICONS, app_icon, banner, PAL, hexrgb

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.abspath(os.path.join(HERE, ".."))
P = lambda *a: os.path.join(OUT, *a)
FONTS = "/usr/share/fonts/truetype/sand-box/google/"
MONO_B = FONTS + "IBM Plex Mono/IBMPlexMono-Bold.ttf"
MONO_M = FONTS + "IBM Plex Mono/IBMPlexMono-Medium.ttf"


def icons():
    os.makedirs(P("icons"), exist_ok=True)
    for name, f in ICONS:
        for n in (16, 32):
            g = f(n)
            g.svg(P("icons", f"{name}-{n}.svg"), title=f"{name} ({n}px)")
            g.image(1).save(P("icons", f"{name}-{n}.png"))


def banners():
    os.makedirs(P("banner"), exist_ok=True)
    g = banner()
    g.image(2).save(P("banner", "wizard-banner.png"))        # 164 x 314
    g.image(4).save(P("banner", "wizard-banner@2x.png"))     # 328 x 628
    g.svg(P("banner", "wizard-banner.svg"), title="Flashwright wizard banner", scale=2)


def appicon():
    os.makedirs(P("app-icon"), exist_ok=True)
    g16, g24, g32 = app_icon(16), app_icon(24), app_icon(32)
    g32.svg(P("app-icon", "app-icon.svg"), title="Flashwright app icon (concept)")
    g24.svg(P("app-icon", "app-icon-24.svg"), title="Flashwright app icon, 24px drawing")
    g16.svg(P("app-icon", "app-icon-16.svg"), title="Flashwright app icon, 16px drawing")
    # integer-scaled frames only; each size picks the closest hand-drawn grid
    frames = {16: g16.image(1), 24: g24.image(1), 32: g32.image(1), 48: g24.image(2),
              64: g32.image(2), 256: g32.image(8), 512: g32.image(16)}
    for s in (16, 32, 48, 256, 512):
        frames[s].save(P("app-icon", f"app-icon-{s}.png"))
    ico = [frames[s] for s in (16, 24, 32, 48, 64, 256)]
    ico[-1].save(P("app-icon", "app-icon.ico"), format="ICO",
                 sizes=[(s, s) for s in (16, 24, 32, 48, 64, 256)], append_images=ico[:-1])


# ---------------- preview sheet ----------------
def tracked(d, xy, text, font, fill, track=0.04):
    x, y = xy
    for ch in text:
        d.text((x, y), ch, font=font, fill=fill)
        x += font.getlength(ch) + track * font.size
    return x


def sheet():
    W, H = 1600, 1440
    im = Image.new("RGB", (W, H), PAL["W"])
    d = ImageDraw.Draw(im)
    fb, fh = ImageFont.truetype(MONO_B, 30), ImageFont.truetype(MONO_B, 18)
    fm, fs = ImageFont.truetype(MONO_M, 15), ImageFont.truetype(MONO_M, 13)
    K, DT, S, WH = hexrgb(PAL["K"]), hexrgb(PAL["D"]), hexrgb(PAL["S"]), hexrgb(PAL["W"])
    paste = lambda img, xy: im.paste(img, xy, img if img.mode == "RGBA" else None)
    tracked(d, (48, 36), "FLASHWRIGHT  /  PHASE 1 BRAND ASSETS", fb, K)
    d.text((48, 82), "Clatty Works · Setup Wizard direction · all pixel art drawn in-house",
           font=fm, fill=DT)
    d.rectangle([48, 112, W - 48, 113], fill=K)

    # --- banner (left column) ---
    bx, by = 48, 150
    tracked(d, (bx, by), "WIZARD BANNER", fh, K)
    b1, b2 = Image.open(P("banner", "wizard-banner.png")), Image.open(P("banner", "wizard-banner@2x.png"))
    paste(b2, (bx, by + 40)); paste(b1, (bx + b2.width + 24, by + 40))
    d.text((bx, by + 50 + b2.height), "@2x  328 x 628", font=fs, fill=DT)
    d.text((bx + b2.width + 24, by + 50 + b1.height), "1x  164 x 314", font=fs, fill=DT)

    # --- app icon ---
    ax, ay = 620, 150
    tracked(d, (ax, ay), "APP ICON  (NO LETTERING IN THE ART)", fh, K)
    d.rectangle([ax, ay + 40, ax + 271, ay + 311], fill=PAL["G"], outline=S)
    paste(Image.open(P("app-icon", "app-icon-256.png")), (ax + 8, ay + 48))
    d.text((ax, ay + 320), "256  (32px master x8)", font=fs, fill=DT)
    sx, y = ax + 300, ay + 40
    sizes = [(16, Image.open(P("app-icon", "app-icon-16.png"))), (24, app_icon(24).image(1)),
             (32, Image.open(P("app-icon", "app-icon-32.png"))), (48, Image.open(P("app-icon", "app-icon-48.png")))]
    for bg, label in ((PAL["W"], "white"), (PAL["G"], "Panel Grey"), (PAL["K"], "Charcoal"), (PAL["D"], "Deep Teal")):
        d.rectangle([sx, y, W - 48, y + 64], fill=bg, outline=S)
        cx = sx + 16
        for s_, ic in sizes:
            paste(ic, (cx, y + 32 - s_ // 2)); cx += s_ + 24
        d.text((cx + 16, y + 24), f"16 / 24 / 32 / 48 at real size on {label}", font=fs,
               fill=K if bg in (PAL["W"], PAL["G"]) else WH)
        y += 70
    zy = ay + 360
    d.text((ax, zy), "16px simplified drawing x8", font=fs, fill=DT)
    paste(app_icon(16).image(8), (ax, zy + 24))
    d.text((ax + 290, zy), "24px drawing x6  (48px = x2)", font=fs, fill=DT)
    paste(app_icon(24).image(6), (ax + 290, zy + 24))
    notes = ["app-icon.ico frames: 16, 24, 32, 48, 64, 256",
             "PNG: 16, 32, 48, 256, 512",
             "SVG: 32px master + 16px and 24px drawings",
             "Whole-number nearest-neighbour scaling only"]
    for i, t in enumerate(notes):
        d.text((ax + 600, zy + 24 + i * 24), t, font=fs, fill=K)

    # --- icon set ---
    iy = 720
    tracked(d, (ax, iy), "ICON SET  (16 + 32 PX)", fh, K)
    cw, ch = 186, 172
    for i, (name, f) in enumerate(ICONS):
        x, y = ax + (i % 5) * cw, iy + 40 + (i // 5) * ch
        d.rectangle([x, y, x + 111, y + 111], fill=PAL["G"], outline=S)
        paste(f(32).image(3), (x + 8, y + 8))
        paste(f(32).image(1), (x + 120, y))
        paste(f(16).image(1), (x + 120, y + 40))
        paste(f(16).image(2), (x + 120, y + 64))
        d.text((x, y + 120), name, font=fs, fill=K)
    d.text((ax, iy + 40 + 2 * ch), "Each: 32px x3 on Panel Grey · 32px and 16px at real size · 16px x2", font=fs, fill=DT)

    # --- app lockup ---
    ly = 1140
    tracked(d, (48, ly), "APP LOCKUP  (HORIZONTAL)", fh, K)
    L = lockup.layout()
    for i, (bg, mono, label) in enumerate(((PAL["W"], False, "on white  (also transparent)"),
                                           (PAL["D"], True, "all-white on Deep Teal"))):
        tmp = P("lockup", f".sheet-tmp-{i}.png")
        lockup.png(L, tmp, 3, bg=bg, mono=mono)
        x = 48 + i * 640
        im.paste(Image.open(tmp), (x, ly + 40)); os.remove(tmp)
        d.rectangle([x, ly + 40, x + 599, ly + 189], outline=S)
        d.text((x, ly + 196), f"{label} · shown at 600 wide, exports at 800 and 1600", font=fs, fill=DT)
    # concept stamp
    d.rectangle([W - 48 - 420, 30, W - 48, 72], fill=PAL["A"], outline=K)
    tracked(d, (W - 48 - 404, 40), "CONCEPT · NOT YET APPROVED BY JAY", fs, K, 0.06)

    d.rectangle([48, H - 60, W - 48, H - 59], fill=K)
    d.text((48, H - 44), "Palette #0F8A8A #0A5C5C #D5D9DC #FFFFFF #2A2E31 #E39B2D · bevels #F2F4F5 #8E959A · IBM Plex (OFL-1.1)",
           font=fs, fill=K)
    tracked(d, (W - 48 - 175, H - 48), "CLATTY WORKS", fh, K)
    im.save(P("flashwright-assets-sheet.png"))


def shots():
    chrome = shutil.which("google-chrome") or shutil.which("chromium") or shutil.which("chromium-browser")
    os.makedirs(P("screenshots"), exist_ok=True)
    pages = [("connect", "01-connect"), ("choose", "02-choose"), ("flashing", "03-flashing"), ("components", "04-components")]
    for frag, name in pages:
        out = P("screenshots", f"demo-{name}.png")
        subprocess.run([chrome, "--headless=new", "--disable-gpu", "--no-sandbox", "--hide-scrollbars",
                        "--force-device-scale-factor=1", "--window-size=1280,800",
                        f"--screenshot={out}", f"file://{P('demo.html')}#{frag}"],
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def check_palette():
    """Fail the build if any exported pixel-art PNG uses a colour outside the 8 brand colours."""
    import glob
    allowed = {hexrgb(c) for c in PAL.values()}
    for f in glob.glob(P("icons", "*.png")) + glob.glob(P("app-icon", "*.png")) + glob.glob(P("banner", "*.png")):
        for _, (r, g, b, a) in Image.open(f).convert("RGBA").getcolors(1 << 20):
            if a and (r, g, b) not in allowed:
                raise SystemExit(f"off-palette colour {(r, g, b)} in {f}")


if __name__ == "__main__":
    icons(); banners(); appicon(); check_palette(); lockup.build(P("lockup")); sheet()
    if "--shots" in sys.argv:
        shots()
    print("built into", OUT)
