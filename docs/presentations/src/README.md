# Presentation source

`../OpenPulseHF-Overview.odp` is **generated**, not hand-edited. Regenerate with:

```sh
cd docs/presentations/src
python3 gen.py
```

- `slides.py` — all slide text and the project URL. Edit here.
- `diagrams.py` — the four native-ODF figures (evidence stack, fade bars, layer blocks, rate
  ladder) plus their styles. Every number in it is quoted from the repository, with the source
  named per diagram.
- `gen.py` — ODF emitter. Writes the `.odp` to the repo root path set at the bottom.
- `qr.txt` / `matrix.txt` are intermediates; they are not committed.

## Notes

**The QR is the maintainer's own image**, embedded as `Pictures/qr.png` from `qr-provided.png`. It
is stylised and points at the project site. It replaced a vector QR the generator used to draw from
`qrencode` output — a QR a room full of people will scan should be the one its owner validated,
not one regenerated from a URL string that can drift out of step with the slide text.

**Its payload has not been verified on this host.** The image is stylised (a coloured centre
element) which defeats naive matrix extraction, and no QR decoder is installed. If one ever is,
check the payload against `URL` in `slides.py` rather than assuming they agree.


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
