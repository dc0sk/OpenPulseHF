#!/usr/bin/env bash
# Render selected slides to PNG. Usage: render.sh 5 24 31
set -u
cd /home/dc0sk/git/OpenPulseHF
WANT="$*"
python3 - "$WANT" <<'PY'
import zipfile, re, os, shutil, sys
want = {int(x) for x in sys.argv[1].split()} if sys.argv[1].strip() else None
SRC=os.environ.get("DECK", "docs/presentations/OpenPulseHF-Overview.odp"); OUTD="target/deckrender/single"
shutil.rmtree(OUTD, ignore_errors=True); os.makedirs(OUTD)
z=zipfile.ZipFile(SRC); parts={n:z.read(n) for n in z.namelist()}
c=parts["content.xml"].decode(); head,tail=c.split("<draw:page ",1)
pages=["<draw:page "+p for p in ("<draw:page "+tail).split("<draw:page ")[1:]]
m=re.search(r'(</draw:page>)(?!.*</draw:page>)', c, re.S); close=c[m.end():]
for i,p in enumerate(pages,1):
    if want and i not in want: continue
    # The LAST page carries the document's closing tags with it; appending `close` again made a
    # malformed file that LibreOffice refused to load — so slide 34 silently never rendered and the
    # check reported PASS over 33 of 34. Cut each page at its own closing tag.
    k = p.rfind("</draw:page>")
    body = p[:k] + "</draw:page>" if k != -1 else p + "</draw:page>"
    q=dict(parts); q["content.xml"]=(head+body+close).encode()
    with zipfile.ZipFile(os.path.join(OUTD,f"s{i:02d}.odp"),"w") as zo:
        zo.writestr(zipfile.ZipInfo("mimetype"), q.pop("mimetype"), zipfile.ZIP_STORED)
        for n,d in q.items(): zo.writestr(n,d,zipfile.ZIP_DEFLATED)
PY
rm -rf target/deckrender/out && mkdir -p target/deckrender/out
timeout 600 flatpak run --filesystem=/home/dc0sk/git/OpenPulseHF org.libreoffice.LibreOffice \
  --headless --convert-to 'png:impress_png_Export:{"PixelWidth":{"type":"long","value":1400},"PixelHeight":{"type":"long","value":788}}' \
  --outdir target/deckrender/out target/deckrender/single/*.odp >/dev/null 2>&1
ls target/deckrender/out/
deck=$(stat -c %Y "${DECK:-docs/presentations/OpenPulseHF-Overview.odp}")
for f in target/deckrender/out/*.png; do
  [ "$(stat -c %Y "$f")" -lt "$deck" ] && { echo "STALE: $f predates the deck"; exit 1; }
done
