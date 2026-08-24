import zipfile, html
from slides import URL, FOOTER, USE_CASES, CONTENT_SLIDES

W, H = 28.0, 15.75
MATRIX = [[int(c) for c in ln.strip()] for ln in open("matrix.txt") if ln.strip()]

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
    """Vector QR from qrencode's matrix — sharp at any projector resolution,
    with horizontal runs merged so the slide holds dozens of rects, not 450."""
    n, quiet = len(MATRIX), 4
    u = size / (n + 2 * quiet)
    o = [f'<draw:rect draw:style-name="qrbg" svg:x="{x}cm" svg:y="{y}cm" '
         f'svg:width="{size}cm" svg:height="{size}cm"/>']
    for r, row in enumerate(MATRIX):
        c = 0
        while c < n:
            if row[c]:
                run = 1
                while c + run < n and row[c + run]: run += 1
                o.append(f'<draw:rect draw:style-name="qrfg" '
                         f'svg:x="{x+(quiet+c)*u:.4f}cm" svg:y="{y+(quiet+r)*u:.4f}cm" '
                         f'svg:width="{run*u:.4f}cm" svg:height="{u:.4f}cm"/>')
                c += run
            else: c += 1
    return "".join(o)

def band(h): return ('<draw:frame draw:style-name="band" svg:x="0cm" svg:y="0cm" '
                     f'svg:width="{W}cm" svg:height="{h}cm"><draw:text-box/></draw:frame>')

# Agenda leads, then the use cases, then the rest — motivation after structure.
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
    p = [f'<draw:page draw:name="S{i}" draw:master-page-name="Default">',
         f'<draw:frame draw:style-name="{band_style}" svg:x="0cm" svg:y="0cm" '
         f'svg:width="{W}cm" svg:height="2.15cm"><draw:text-box/></draw:frame>',
         tbox(1.4, 0.42, W-2.8, 1.4, head_style, [title]),
         tbox(1.6, 2.8, W-3.2, H-4.15, "Body", body),
         tbox(1.6, H-1.12, 18.5, 0.7, "Foot", [FOOTER]),
         tbox(W-4.0, H-1.12, 2.4, 0.7, "FootR", [f"{i} / {total}"]),
         '</draw:page>']
    pages.append("".join(p))

clo = [f'<draw:page draw:name="Closing" draw:master-page-name="Default">', band(H),
  tbox(2.0, 3.0, 17.0, 2.4, "TitleBig", ["Thank you"]),
  tbox(2.0, 5.9, 17.0, 4.8, "TitleSub",
       ["OpenPulseHF — an open, plugin-based HF software modem", "",
        "Source, issues, design documents and the traceability ledger:", URL, "",
        "Questions welcome — including the awkward ones about what is not done."]),
  tbox(2.0, 11.5, 17.0, 1.6, "CoverMeta",
       ["Simon Keimer (DC0SK)   ·   v0.16.0 (pre-1.0)"]),
  qr(21.4, 5.1, 4.9),
  tbox(21.4, 10.25, 4.9, 0.8, "QrCap", ["Project repository"]),
  tbox(1.6, H-1.12, 18.5, 0.7, "FootLight", [FOOTER]),
  tbox(W-4.0, H-1.12, 2.4, 0.7, "FootRLight", [f"{total} / {total}"]),
  '</draw:page>']
pages.append("".join(clo))

STYLES = '''
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
 '</manifest:manifest>')

import os
# repo-relative: this file lives in docs/presentations/src/
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..",
                   "OpenPulseHF-Overview.odp")
with zipfile.ZipFile(out, "w") as z:
    z.writestr(zipfile.ZipInfo("mimetype"),
               "application/vnd.oasis.opendocument.presentation", zipfile.ZIP_STORED)
    z.writestr("META-INF/manifest.xml", MANIFEST, zipfile.ZIP_DEFLATED)
    z.writestr("content.xml", CONTENT, zipfile.ZIP_DEFLATED)
    z.writestr("styles.xml", MASTER, zipfile.ZIP_DEFLATED)
    z.writestr("meta.xml", META, zipfile.ZIP_DEFLATED)
print(f"wrote {out} — {total} slides "
      f"(cover + {len(USE_CASES)} use cases + {len(CONTENT_SLIDES)} content + closing)")
