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
- `diagrams.py` — the four native-ODF figures (evidence stack, fade bars, layer blocks, rate
  ladder) plus their styles. Every number in it is quoted from the repository, with the source
  named per diagram.
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

## The bounds guard

`gen.py` refuses to write a deck whose shapes leave the 28.0 x 15.75 cm page. This is not
belt-and-braces: **there is no LibreOffice on the build host**, so nothing renders the file and an
overflow would first appear on a projector. Two of the four diagrams overflowed on their first
build and the guard is what found them. Sabotage-verified — pushing a diagram off-page makes
`gen.py` exit non-zero and name the slide.

It checks geometry only. "No shape leaves the page" is not "it looks right"; open the deck before
presenting it.
