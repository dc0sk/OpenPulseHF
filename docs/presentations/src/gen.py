import os
import zipfile, html
from slides import URL, FOOTER, USE_CASES, CONTENT_SLIDES

W, H = 28.0, 15.75
# (The QR module matrix used to be read from matrix.txt here. It went with the vector QR —
# the deck now embeds qr-provided.png, so qrencode is no longer part of the build.)

import diagrams



def check_bounds(xml):
    """Refuse to write a deck whose shapes leave the page.

    There is no LibreOffice on the build host, so nothing renders this and nobody sees an overflow
    until it is on a projector. Two of the four diagrams overflowed on their first build; this is
    the check that found them.
    """
    import re
    bad = []
    for page in re.split(r"<draw:page ", xml)[1:]:
        name = re.search(r'draw:name="([^"]+)"', page)
        name = name.group(1) if name else "?"
        for m in re.finditer(r'svg:x="([\d.]+)cm" svg:y="([\d.]+)cm" '
                             r'svg:width="([\d.]+)cm" svg:height="([\d.]+)cm"', page):
            x, y, w, h = map(float, m.groups())
            if x + w > W + 0.01 or y + h > H + 0.01:
                bad.append(f"{name}: extends to x={x+w:.2f} y={y+h:.2f} (page {W}x{H})")
    if bad:
        raise SystemExit("slide geometry leaves the page:\n  " + "\n  ".join(bad))


def esc(t): return html.escape(t, quote=False)

def tbox(x, y, w, h, para, lines):
    o = [f'<draw:frame draw:style-name="clear" svg:x="{x}cm" svg:y="{y}cm" '
         f'svg:width="{w}cm" svg:height="{h}cm"><draw:text-box>']
    for ln in lines:
        o.append(f'<text:p text:style-name="{para}"/>' if ln == ""
                 else f'<text:p text:style-name="{para}">{esc(ln)}</text:p>')
    o.append('</draw:text-box></draw:frame>')
    return "".join(o)

def qr(x, y, size):
    """Place the maintainer's QR image.

    This used to DRAW a vector QR from `qrencode`'s matrix. It is now the supplied
    `qr-provided.png`, because that one is stylised and points at the project site — and because a
    QR a room full of people will scan should be the one its owner validated, not one regenerated
    from a URL string that could drift.

    NOT independently verified here: the image is stylised (a coloured centre element) and no QR
    decoder is installed on this host, so the payload is taken on the maintainer's validation. If a
    decoder ever lands, check it against URL above rather than assuming.
    """
    return (f'<draw:frame draw:style-name="clear" svg:x="{x}cm" svg:y="{y}cm" '
            f'svg:width="{size}cm" svg:height="{size}cm">'
            f'<draw:image xlink:href="Pictures/qr.png" xlink:type="simple" '
            f'xlink:show="embed" xlink:actuate="onLoad"/></draw:frame>')


def band(h): return ('<draw:frame draw:style-name="band" svg:x="0cm" svg:y="0cm" '
                     f'svg:width="{W}cm" svg:height="{h}cm"><draw:text-box/></draw:frame>')

# Agenda leads, then the use cases, then the rest — motivation after structure.
LEDES = {
    "Where it sits": "One binary, seven layers — and exactly one of them is a plugin boundary.",
    "Difference 2 — the ladder adapts on EVIDENCE":
        "Fourteen rungs. It climbs on decodes, not on an SNR estimate that a fade can flatten.",
    "Testing — six layers":
        "Each layer is cheaper than the one above it, and proves less. Both halves matter.",
    "Performance — measured on simulated channels":
        "Decode rate on a Watterson moderate_f1 fade. The zeros are the interesting part.",
}

DIAGRAMS = {
    "Where it sits": diagrams.stack_blocks,
    "Difference 2 — the ladder adapts on EVIDENCE": diagrams.ladder_steps,
    "Testing — six layers": diagrams.evidence_stack,
    "Performance — measured on simulated channels": diagrams.fade_bars,
}

AGENDA, REST = CONTENT_SLIDES[0], CONTENT_SLIDES[1:]
ALL = ([("c", *AGENDA)]
       + [("usecase", t, b) for t, b in USE_CASES]
       + [("c", t, b) for t, b in REST])
total = len(ALL) + 2
pages = []

cov = [f'<draw:page draw:name="Cover" draw:master-page-name="Default">', band(H),
  tbox(2.0, 3.3, 17.0, 2.8, "TitleBig", ["OpenPulseHF"]),
  tbox(2.0, 6.2, 17.0, 3.4, "TitleSub",
       ["An open, plugin-based HF software modem", "",
        "Architecture · Implementation · Verification · Status"]),
  tbox(2.0, 10.6, 17.0, 2.4, "CoverMeta",
       ["Simon Keimer (DC0SK)   ·   v0.16.0 (pre-1.0)", URL]),
  qr(21.4, 5.1, 4.9),
  tbox(21.4, 10.25, 4.9, 0.8, "QrCap", ["Project repository"]),
  '</draw:page>']
pages.append("".join(cov))

for i, (kind, title, body) in enumerate(ALL, start=2):
    head_style = "HeadAccent" if kind == "usecase" else "Head"
    band_style = "bandAccent" if kind == "usecase" else "band"
    fig = DIAGRAMS.get(title)
    p = [f'<draw:page draw:name="S{i}" draw:master-page-name="Default">',
         f'<draw:frame draw:style-name="{band_style}" svg:x="0cm" svg:y="0cm" '
         f'svg:width="{W}cm" svg:height="2.15cm"><draw:text-box/></draw:frame>',
         tbox(1.4, 0.42, W-2.8, 1.4, head_style, [title])]
    if fig:
        # The figure replaces the bullets. body[0] is NOT a lede — on these slides it was the first
        # LIST ITEM, and using it produced "1 Unit 183 files with inline tests" as an intro. Each
        # diagram slide names its own one-line lede instead.
        p.append(tbox(1.6, 2.42, W-3.2, 1.0, "Lede", [LEDES.get(title, "")]))
        p.append(fig(tbox))
    else:
        p.append(tbox(1.6, 2.8, W-3.2, H-4.15, "Body", body))
    p += [
         tbox(1.6, H-1.12, 18.5, 0.7, "Foot", [FOOTER]),
         tbox(W-4.0, H-1.12, 2.4, 0.7, "FootR", [f"{i} / {total}"]),
         '</draw:page>']
    pages.append("".join(p))

clo = [f'<draw:page draw:name="Closing" draw:master-page-name="Default">', band(H),
  tbox(2.0, 3.0, 17.0, 2.4, "TitleBig", ["Thank you"]),
  # 19 cm, not 17: at 18 pt the old width wrapped every line, and the last one then collided with
  # the footer. Lines are kept short enough to sit on one line each at this width.
  tbox(2.0, 5.7, 19.0, 5.6, "TitleSub",
       ["An open, plugin-based HF software modem", "",
        "Source, issues, design docs and the ledger:", URL, "",
        "Questions welcome — including the awkward ones."]),
  # No name/version line here: the footer below already reads "an open-source project by Simon
  # Keimer (DC0SK)", so repeating it mid-slide was redundant. Removed by the maintainer directly in
  # the .odp and folded back here, so the next regeneration keeps the change instead of undoing it.
  qr(21.4, 5.1, 4.9),
  tbox(21.4, 10.25, 4.9, 0.8, "QrCap", ["Project repository"]),
  tbox(1.6, H-1.12, 18.5, 0.7, "FootLight", [FOOTER]),
  tbox(W-4.0, H-1.12, 2.4, 0.7, "FootRLight", [f"{total} / {total}"]),
  '</draw:page>']
pages.append("".join(clo))

STYLES = diagrams.DIAGRAM_STYLES + '''
<style:style style:name="band" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#12304a" draw:stroke="none"/></style:style>
<style:style style:name="bandAccent" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#0d5c63" draw:stroke="none"/></style:style>
<style:style style:name="qrbg" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#ffffff" draw:stroke="none"/></style:style>
<style:style style:name="qrfg" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#12304a" draw:stroke="none"/></style:style>
<style:style style:name="clear" style:family="graphic"><style:graphic-properties
 draw:fill="none" draw:stroke="none" draw:auto-grow-height="true" fo:padding="0cm"/></style:style>
<style:style style:name="TitleBig" style:family="paragraph"><style:text-properties
 fo:font-size="52pt" fo:font-weight="bold" fo:color="#ffffff"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="TitleSub" style:family="paragraph">
 <style:paragraph-properties fo:line-height="142%"/><style:text-properties
 fo:font-size="18pt" fo:color="#cfe0ee" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="CoverMeta" style:family="paragraph">
 <style:paragraph-properties fo:line-height="150%"/><style:text-properties
 fo:font-size="15pt" fo:color="#8fb4d0" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="QrCap" style:family="paragraph">
 <style:paragraph-properties fo:text-align="center"/><style:text-properties
 fo:font-size="11pt" fo:color="#8fb4d0" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="Head" style:family="paragraph"><style:text-properties
 fo:font-size="26pt" fo:font-weight="bold" fo:color="#ffffff"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="HeadAccent" style:family="paragraph"><style:text-properties
 fo:font-size="26pt" fo:font-weight="bold" fo:color="#ffffff"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="Body" style:family="paragraph">
 <style:paragraph-properties fo:line-height="130%"/><style:text-properties
 fo:font-size="15.5pt" fo:color="#1c1c1c" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="Foot" style:family="paragraph"><style:text-properties
 fo:font-size="9.5pt" fo:color="#7d8b96" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="FootR" style:family="paragraph">
 <style:paragraph-properties fo:text-align="end"/><style:text-properties
 fo:font-size="9.5pt" fo:color="#7d8b96" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="FootLight" style:family="paragraph"><style:text-properties
 fo:font-size="9.5pt" fo:color="#7f9db4" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="FootRLight" style:family="paragraph">
 <style:paragraph-properties fo:text-align="end"/><style:text-properties
 fo:font-size="9.5pt" fo:color="#7f9db4" style:font-name="Liberation Sans"/></style:style>
'''

NS = ('xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
 'xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" '
 'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
 'xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" '
 'xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" '
 'xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" '
 'xmlns:xlink="http://www.w3.org/1999/xlink" '
 'xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" ')

CONTENT = ('<?xml version="1.0" encoding="UTF-8"?><office:document-content ' + NS +
 'office:version="1.2">'
 f'<office:automatic-styles>{STYLES}</office:automatic-styles>'
 '<office:body><office:presentation>' + "".join(pages) +
 '</office:presentation></office:body></office:document-content>')

MASTER = ('<?xml version="1.0" encoding="UTF-8"?><office:document-styles ' + NS +
 'office:version="1.2"><office:automatic-styles><style:page-layout style:name="PM1">'
 f'<style:page-layout-properties fo:page-width="{W}cm" fo:page-height="{H}cm" '
 'style:print-orientation="landscape" fo:margin-top="0cm" fo:margin-bottom="0cm" '
 'fo:margin-left="0cm" fo:margin-right="0cm"/></style:page-layout>'
 '</office:automatic-styles><office:master-styles><style:master-page '
 'style:name="Default" style:page-layout-name="PM1"/></office:master-styles>'
 '</office:document-styles>')

META = ('<?xml version="1.0" encoding="UTF-8"?><office:document-meta '
 'xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
 'xmlns:dc="http://purl.org/dc/elements/1.1/" office:version="1.2"><office:meta>'
 '<dc:title>OpenPulseHF — Architecture, Implementation, Verification</dc:title>'
 '<dc:creator>Simon Keimer (DC0SK)</dc:creator>'
 '<dc:subject>An open, plugin-based HF software modem</dc:subject>'
 '</office:meta></office:document-meta>')

MANIFEST = ('<?xml version="1.0" encoding="UTF-8"?><manifest:manifest '
 'xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2">'
 '<manifest:file-entry manifest:full-path="/" '
 'manifest:media-type="application/vnd.oasis.opendocument.presentation"/>'
 '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>'
 '<manifest:file-entry manifest:full-path="styles.xml" manifest:media-type="text/xml"/>'
 '<manifest:file-entry manifest:full-path="meta.xml" manifest:media-type="text/xml"/>'
 '<manifest:file-entry manifest:full-path="Pictures/qr.png" manifest:media-type="image/png"/>'
 '</manifest:manifest>')

import os
# repo-relative: this file lives in docs/presentations/src/
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..",
                   "OpenPulseHF-Overview.odp")
check_bounds(CONTENT)
with zipfile.ZipFile(out, "w") as z:
    z.writestr(zipfile.ZipInfo("mimetype"),
               "application/vnd.oasis.opendocument.presentation", zipfile.ZIP_STORED)
    z.writestr("META-INF/manifest.xml", MANIFEST, zipfile.ZIP_DEFLATED)
    z.writestr("content.xml", CONTENT, zipfile.ZIP_DEFLATED)
    z.writestr("styles.xml", MASTER, zipfile.ZIP_DEFLATED)
    z.writestr("meta.xml", META, zipfile.ZIP_DEFLATED)
    qr_png = os.path.join(os.path.dirname(os.path.abspath(__file__)), "qr-provided.png")
    with open(qr_png, "rb") as f:
        z.writestr("Pictures/qr.png", f.read(), zipfile.ZIP_STORED)
print(f"wrote {out} — {total} slides "
      f"(cover + {len(USE_CASES)} use cases + {len(CONTENT_SLIDES)} content + closing)")
