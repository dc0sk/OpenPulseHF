# Presentation source

`../OpenPulseHF-Overview.odp` is **generated**, not hand-edited. Regenerate with:

```sh
cd docs/presentations/src
qrencode -t ASCII -m 0 -l M "https://github.com/dc0sk/OpenPulseHF" > qr.txt
python3 -c "
lines=[l.rstrip('\n') for l in open('qr.txt') if l.strip('\n')]
rows=[[1 if l[i]=='#' else 0 for i in range(0,len(l),2)] for l in lines]
open('matrix.txt','w').write('\n'.join(''.join(str(v) for v in r) for r in rows))"
python3 gen.py
```

- `slides.py` — all slide text. Edit here.
- `gen.py` — ODF emitter. Writes the `.odp` to the repo root path set at the bottom.
- `qr.txt` / `matrix.txt` are intermediates; they are not committed.

## Notes

**The QR is vector, not an image.** `qrencode` produces the module matrix and `gen.py`
draws it as native ODF rectangles with horizontal runs merged (~444 rects). It stays
sharp at any projector resolution and carries no image-codec dependency.

`qr-provided.png` is a validated QR supplied by the maintainer, kept as an alternative
asset. The generated deck does not reference it.

**A hand-rolled QR encoder was written and discarded.** Round-tripping it showed its
Reed-Solomon generator polynomial was coefficient-reversed: the payload decoded but the
error-correction bytes were wrong, so a real scanner would have failed or mis-corrected.
It had also assumed version 4 where `qrencode` correctly selects version 3. Use the tool.

## Editing by hand

Opening the `.odp` in LibreOffice and saving works fine — but the next `gen.py` run
overwrites it. Either keep edits in `slides.py`, or stop regenerating once hand-editing
starts.
