"""Detect slides whose text overflows the page, by looking at the RENDER.

Why this exists: `gen.py`'s bounds guard proves no SHAPE leaves the page, and that is not the same
property. A text box sits comfortably inside the page while its TEXT overflows the box — LibreOffice
then clips it, and the audience simply never sees the last lines. The regulatory slide shipped that
way and only a render showed it.

Method: render each slide to PNG, then look for ink in the strip BELOW the footer baseline. Nothing
is laid out there by design, so ink there means text ran past where it should stop.

Usage:  python3 check_render.py <dir-of-slide-pngs>
"""
import sys, zlib, struct, os, re


def png_gray(path):
    """Minimal PNG reader — 8-bit RGB/RGBA/grey only. Returns (w, h, luminance(x, y))."""
    d = open(path, "rb").read()
    pos, idat = 8, b""
    while pos < len(d):
        ln = struct.unpack(">I", d[pos:pos + 4])[0]
        typ = d[pos + 4:pos + 8]
        body = d[pos + 8:pos + 8 + ln]
        pos += 12 + ln
        if typ == b"IHDR":
            w, h, bitd, colt = struct.unpack(">IIBB", body[:10])
        elif typ == b"IDAT":
            idat += body
        elif typ == b"IEND":
            break
    if bitd != 8:
        raise SystemExit(f"{path}: only 8-bit PNGs supported (got {bitd})")
    ch = {0: 1, 2: 3, 4: 2, 6: 4}[colt]
    stride, bpp = w * ch, ch
    raw = zlib.decompress(idat)
    out, prev, i = bytearray(), bytearray(stride), 0
    for _ in range(h):
        f = raw[i]; i += 1
        line = bytearray(raw[i:i + stride]); i += stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1: line[x] = (line[x] + a) & 255
            elif f == 2: line[x] = (line[x] + b) & 255
            elif f == 3: line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if (pa <= pb and pa <= pc) else (b if pb <= pc else c)
                line[x] = (line[x] + pr) & 255
        out += line; prev = line

    def lum(x, y):
        o = y * stride + x * ch
        if ch <= 2:
            return out[o]
        return (out[o] * 299 + out[o + 1] * 587 + out[o + 2] * 114) // 1000

    return w, h, lum


def main(d):
    files = sorted(f for f in os.listdir(d) if f.endswith(".png"))
    if not files:
        raise SystemExit(f"no PNGs in {d} — nothing was checked, which is not the same as 'no problems'")
    bad = []
    for f in files:
        w, h, lum = png_gray(os.path.join(d, f))
        # The footer baseline sits at 14.63/15.75 of the height; anything below is spillover.
        # Full-bleed dark slides (cover, closing) are dark everywhere BY DESIGN. Measuring absolute
        # ink there flags them every time — the first version did, at 28 700 "overflow" pixels on the
        # cover. Skip any slide that is mostly dark: the test is for ink where the background is pale.
        dark_frac = sum(1 for y in range(0, h, 9) for x in range(0, w, 9) if lum(x, y) < 110) / \
                    max(1, len(range(0, h, 9)) * len(range(0, w, 9)))
        if dark_frac > 0.4:
            continue
        y0 = int(h * 14.95 / 15.75)
        ink = sum(1 for y in range(y0, h) for x in range(0, w, 2) if lum(x, y) < 110)
        if ink > 40:
            bad.append((f, ink))
    for f, ink in bad:
        print(f"  OVERFLOW {f}: {ink} dark pixels below the footer baseline")
    print(f"RENDER-CHECK: {'FAIL' if bad else 'PASS'} — {len(files)} slides, {len(bad)} overflowing")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else "target/deckrender/out"))
