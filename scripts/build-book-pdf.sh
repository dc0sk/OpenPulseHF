#!/usr/bin/env bash
# Render docs/openpulse-book.md to a typeset PDF.
#
# Pipeline: pandoc (GFM -> HTML) -> WeasyPrint (HTML+CSS -> PDF), styled by scripts/book-pdf.css.
#
# Why not LaTeX: this book is mostly wide tables and ASCII block diagrams. CSS gives direct control
# over how those degrade (tables wrap at a reduced size; diagrams shrink but never wrap), and the
# xelatex route additionally needs framed/fvextra/titlesec, which a minimal TeX install lacks.
#
# Requires: pandoc, weasyprint, python3, and the DejaVu fonts (the book uses Greek, maths operators
# and box-drawing characters; a font without box drawing turns every diagram into tofu).
#
# Usage: scripts/build-book-pdf.sh [output.pdf]
set -euo pipefail

cd "$(dirname "$0")/.."

SRC=docs/openpulse-book.md
OUT="${1:-docs/openpulse-book.pdf}"
CSS=scripts/book-pdf.css

for tool in pandoc weasyprint python3; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found" >&2; exit 1; }
done
for f in "$SRC" "$CSS"; do
  [[ -f "$f" ]] || { echo "error: $f not found" >&2; exit 1; }
done
fc-list | grep -qi "DejaVuSansMono" || echo "warning: DejaVu Sans Mono not found; ASCII diagrams may not render" >&2

VERSION="$(awk '
  /^\[workspace\.package\]/ { in_section=1; next }
  /^\[/ { in_section=0 }
  in_section && $1 == "version" && $2 == "=" { gsub(/"/, "", $3); print $3; exit }
' Cargo.toml)"
BUILD_DATE="$(date -u -d "@${SOURCE_DATE_EPOCH:-$(date +%s)}" +%Y-%m-%d)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Strip frontmatter, the H1 title and the markdown TOC (WeasyPrint generates one with page numbers).
python3 - "$SRC" "$WORK/book.md" <<'PY'
import re, sys
s = open(sys.argv[1]).read()
s = re.sub(r'\A---\n.*?\n---\n', '', s, flags=re.S)
s = re.sub(r'\A# .*?\n', '', s)
s = re.sub(r'\n## Table of contents\n.*?(?=\n---\n)', '', s, flags=re.S)
open(sys.argv[2], 'w').write(s.lstrip())
PY

pandoc "$WORK/book.md" \
  --from=gfm \
  --to=html5 \
  --standalone \
  --highlight-style=kate \
  --shift-heading-level-by=-1 \
  -o "$WORK/book.html"

# Build the title page, generate the TOC from real headings, and tag oversize tables/code so the
# stylesheet can scale them down rather than let them overflow the page box.
python3 - "$WORK/book.html" "$VERSION" "$BUILD_DATE" <<'PY'
import re, sys, html
path, version, date = sys.argv[1], sys.argv[2], sys.argv[3]
s = open(path).read()
# Drop pandoc's own title block; the script builds a proper title page below.
s = re.sub(r'<header id="title-block-header">.*?</header>', '', s, flags=re.S)

def slug(i, txt):
    a = re.sub(r'<[^>]+>', '', txt)
    a = re.sub(r'[^\w\s-]', '', html.unescape(a)).strip().lower()
    return re.sub(r'\s+', '-', a) or f'sec{i}'

# Give every heading a stable id and collect the TOC.
toc, n = [], 0
def tag(m):
    global n
    n += 1
    lvl, attrs, txt = int(m.group(1)), m.group(2), m.group(3)
    sid = re.search(r'id="([^"]+)"', attrs)
    sid = sid.group(1) if sid else slug(n, txt)
    if 'id=' not in attrs:
        attrs = f' id="{sid}"' + attrs
    if lvl <= 3:
        toc.append((lvl, sid, re.sub(r'<[^>]+>', '', txt)))
    return f'<h{lvl}{attrs}>{txt}</h{lvl}>'
s = re.sub(r'<h([1-4])([^>]*)>(.*?)</h\1>', tag, s, flags=re.S)

items, cur = [], 0
for lvl, sid, txt in toc:
    while cur < lvl - 1: items.append('<ul>'); cur += 1
    while cur > lvl - 1: items.append('</ul>'); cur -= 1
    items.append(f'<li><a href="#{sid}">{html.escape(txt)}</a></li>')
while cur > 0: items.append('</ul>'); cur -= 1
toc_html = ('<section id="toc"><h1 class="nobreak">Contents</h1><ul>'
            + ''.join(items) + '</ul></section>')

title_html = f'''<section class="titlepage">
<h1>The OpenPulseHF Book</h1>
<p class="subtitle">An amateur-radio HF software modem:<br>waveforms, physics, software, and operation</p>
<p class="meta">
<strong>Release {html.escape(version)}</strong> &nbsp;·&nbsp; built {html.escape(date)}<br>
Written for licensed amateur operators, electronic engineers and software developers.<br>
Every technical claim is traceable to a file, symbol, test or measured result in the repository
at this release; measured figures carry their conditions, and anything unverified is labelled.
</p>
</section>'''

s = s.replace('<body>', '<body>' + title_html + toc_html, 1)

# Scale oversize tables: fixed layout plus a smaller face beats horizontal overflow.
def size_table(m):
    t = m.group(0)
    cols = len(re.findall(r'<th\b', t.split('</thead>')[0])) if '</thead>' in t else 0
    longest = max((len(re.sub(r'<[^>]+>', '', c)) for c in re.findall(r'<td\b[^>]*>(.*?)</td>', t, re.S)), default=0)
    cls = 'verywide' if (cols >= 6 or longest > 160) else ('wide' if (cols >= 5 or longest > 90) else '')
    return t.replace('<table>', f'<table class="{cls}">', 1) if cls else t
s = re.sub(r'<table>.*?</table>', size_table, s, flags=re.S)

# Scale each code block to its own widest line. A single fixed size either clips the wide ASCII
# diagrams or makes every ordinary block needlessly tiny; wrapping is not an option because a wrapped
# diagram is unreadable. Column is ~172mm; DejaVu Sans Mono advance is 0.602em.
COL_MM, ADV, PT_MM = 171.0, 0.602, 0.3528
def size_pre(m):
    t = m.group(0)
    body = re.sub(r'<[^>]+>', '', t)
    longest = max((len(html.unescape(l)) for l in body.split('\n')), default=0)
    fs = 7.1 if longest <= 0 else min(7.1, COL_MM / (longest * ADV * PT_MM))
    fs = max(4.9, fs)                      # floor: below this it is unreadable anyway
    cls = ' class="long"' if t.count('\n') > 42 else ''
    return t.replace('<pre', f'<pre{cls} style="font-size:{fs:.2f}pt"', 1)
s = re.sub(r'<pre.*?</pre>', size_pre, s, flags=re.S)

open(path, 'w').write(s)
PY

weasyprint -s "$CSS" "$WORK/book.html" "$OUT"

if command -v pdfinfo >/dev/null; then
  PAGES="$(pdfinfo "$OUT" | awk '/^Pages:/{print $2}')"
else
  PAGES="$(python3 -c "
import re
d = open('$OUT','rb').read()
print(len(re.findall(rb'/Type\\s*/Page(?![s/])', d)) or '?')
")"
fi
echo "Wrote $OUT ($(du -h "$OUT" | cut -f1), ~${PAGES} pages, workspace v${VERSION})"
