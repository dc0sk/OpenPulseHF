"""Native-ODF diagrams for the four content-heaviest slides.

Everything here emits `draw:rect` and `draw:frame` only — the same primitives the QR uses — so a
slide stays sharp at any projector resolution and carries no external asset. Colours and fonts come
from the deck's existing style names; a diagram never invents a palette.

Every NUMBER in this file is quoted from the repository, not from memory. Sources are named per
diagram so a figure can be re-checked when the code moves.
"""

W = 28.0

# The palette, keyed to the deck's bands. Defined once so a diagram cannot drift from the deck.
INK        = "#12304a"   # band navy
ACCENT     = "#0d5c63"   # use-case teal
WARM       = "#8a4b1f"   # "costs more / proves more"
MUTED      = "#8fb4d0"
PAPER      = "#ffffff"
RULE       = "#c9d6e2"


def _rect(x, y, w, h, style):
    return (f'<draw:rect draw:style-name="{style}" svg:x="{x:.2f}cm" svg:y="{y:.2f}cm" '
            f'svg:width="{w:.2f}cm" svg:height="{h:.2f}cm"/>')


def _label(tbox, x, y, w, h, para, lines):
    return tbox(x, y, w, h, para, lines)


def evidence_stack(tbox):
    """Slide 'Testing — six layers': the evidence tiers as a stack.

    Source: CLAUDE.md 'Acceptance criteria' + docs/dev/virtual-loopback.md. The ordering is the
    project's own evidence hierarchy — cheap/fast/proves-little at the base, expensive/slow/
    conclusive at the top. The right-hand column is what each tier CANNOT prove, which is the part
    that makes the hierarchy load-bearing rather than decorative.
    """
    rows = [
        ("on-air",           "real RF, real rigs, real propagation", "slow, weather-dependent, hard to repeat"),
        ("dual-card rig",    "two clocks, real analog path",         "rig state is a live variable"),
        ("virtual loopback", "one clock, no analog path",            "cannot see sample-rate offset"),
        ("channel sim",      "Watterson fade, Gilbert-Elliott burst", "the model is not the ionosphere"),
        ("integration",      "crate boundaries, real seams",          "buffer IS the frame — no search"),
        ("unit",             "2408 tests, seconds to run",            "nothing about the wire"),
    ]
    x0, top, rw, rh, gap = 1.4, 3.55, 12.2, 1.42, 0.20
    out = []
    for i, (name, proves, cannot) in enumerate(rows):
        y = top + i * (rh + gap)
        # Uniform width. An earlier version tapered the boxes, which narrowed the LOWER rows —
        # exactly the ones with the longest labels — and clipped "real seams" and "seconds to run".
        # A shape that cuts its own text off is worse than a plain rectangle.
        out.append(_rect(x0, y, rw, rh,
                         "tierTop" if i == 0 else ("tierMid" if i < 3 else "tierBase")))
        out.append(_label(tbox, x0 + 0.45, y + 0.14, 5.2, rh, "TierName", [name]))
        out.append(_label(tbox, x0 + 5.3, y + 0.22, 6.6, rh, "TierNote", [proves]))
        out.append(_label(tbox, x0 + rw + 0.55, y + 0.22, 9.4, rh, "TierCannot", ["cannot prove: " + cannot]))
    out.append(_label(tbox, x0, top + len(rows) * (rh + gap) + 0.12, 13.0, 1.0, "TierAxis",
                      ["▲  slower, costlier, closer to the air  —  ▼  faster, cheaper, proves less"]))
    return "".join(out)


def fade_bars(tbox):
    """Slide 'Performance': decode rate on a Watterson moderate_f1 fade.

    Source: CLAUDE.md → 'An HF ladder calibrated on AWGN is not an HF ladder' and the #923 entry.
    The point of the figure is NOT that the bars are long — it is that the two ZERO bars are rungs
    the ladder shipped with, and that the fix in each case was FEC or differential encoding rather
    than a better tracker.
    """
    rows = [
        ("BPSK31, uncoded",      0.00, "@3 dB — the rung every session STARTED on", False),
        ("BPSK31 + Rs",          0.25, "@3 dB — coded", True),
        ("BPSK31 + RsStrong",    1.00, "@3 dB — free below 191 B", True),
        (None, None, None, None),
        ("QPSK250, coherent",    0.00, "@20 dB — dead at EVERY SNR to 40 dB", False),
        ("QPSK250-D",            0.65, "@20 dB — differential, hpx_hf SL6", True),
    ]
    x0, top, barx, barw, rh = 1.4, 3.7, 9.6, 11.4, 1.30
    out = []
    y = top
    for r in rows:
        if r[0] is None:
            y += 0.55
            continue
        name, val, note, good = r
        out.append(_label(tbox, x0, y + 0.06, 8.0, rh, "BarName", [name]))
        out.append(_rect(barx, y + 0.16, barw, 0.86, "barTrack"))
        if val > 0:
            out.append(_rect(barx, y + 0.16, barw * val, 0.86, "barGood" if good else "barBad"))
        else:
            # a zero bar must still be visible, or "it decoded nothing" reads as "no data"
            out.append(_rect(barx, y + 0.16, 0.10, 0.86, "barBad"))
        out.append(_label(tbox, barx + barw + 0.35, y + 0.20, 2.2, rh, "BarVal", [f"{val:.2f}"]))
        out.append(_label(tbox, barx, y + 1.00, barw, 0.8, "BarNote", [note]))
        y += rh + 0.30
    out.append(_label(tbox, x0, y + 0.15, 25.0, 1.0, "BarFoot",
                      ["Fraction of frames decoded. Measured on the channel simulator; "
                       "the zeros are what an AWGN-calibrated ladder could not see."]))
    return "".join(out)


def stack_blocks(tbox):
    """Slide 'Where it sits': the layer stack, with what is pluggable marked.

    Source: CLAUDE.md crate map. Only the boxes that correspond to real crates are drawn.
    """
    layers = [
        ("Applications",  "Pat · Winlink · your own client",                    "ext"),
        ("Protocol",      "ARDOP TNC · KISS/AX.25 · B2F · filexfer · QSY · JS8", "own"),
        ("Session",       "HPX state machine · ARQ/HARQ · rate ladder · trust",  "own"),
        ("Modem",         "ModemEngine · scheduler · CSMA/DCD · diagnostics",    "own"),
        ("Waveform",      "16 plugins — BPSK … 64QAM, OFDM, SC-FDMA, MFSK16",    "plug"),
        ("DSP",           "RRC · PLL · Gardner · LMS/DFE · noise floor",         "own"),
        ("I/O",           "CPAL audio · PTT: serial, VOX, rigctld, CM108, GPIO", "own"),
    ]
    x0, top, bw, bh, gap = 3.2, 3.30, 21.6, 1.24, 0.16
    style = {"ext": "blkExt", "own": "blkOwn", "plug": "blkPlug"}
    out = []
    for i, (name, detail, kind) in enumerate(layers):
        y = top + i * (bh + gap)
        out.append(_rect(x0, y, bw, bh, style[kind]))
        out.append(_label(tbox, x0 + 0.55, y + 0.16, 6.0, bh, "BlkName", [name]))
        out.append(_label(tbox, x0 + 6.6, y + 0.24, 14.4, bh, "BlkNote", [detail]))
    y = top + len(layers) * (bh + gap) + 0.06
    out.append(_label(tbox, x0, y, bw, 0.85, "BlkKey",
                      ["Teal = the plugin boundary: a new waveform is a crate, not a fork.  "
                       "Grey = someone else's software talking to us over a standard TNC port."]))
    return "".join(out)


def ladder_steps(tbox):
    """Slide 'the ladder adapts on EVIDENCE': hpx_hf as a staircase.

    Source: docs/mode-fec-ladder.md hpx_hf row + CLAUDE.md #934. Rungs are compressed to the
    families that matter for the talk; the caption says so rather than implying all 14 are drawn.
    """
    # SIX rungs, not seven. Seven gave 3.5 cm columns, and the two longest labels then ran into
    # their neighbours — visible only in a render. Widening the label boxes just moved the collision.
    # Fewer, wider columns is the fix; the caption says families are shown rather than all 14 rungs.
    steps = [
        ("SL1", "MFSK16", "non-coherent"),
        ("SL2", "BPSK31 +FEC", "entry rung"),
        ("SL6", "QPSK250-D", "differential"),
        ("SL8", "OFDM52", "multicarrier"),
        ("SL11", "OFDM52-16QAM", ""),
        ("SL14", "64QAM r\u22488/9", "top rung"),
    ]
    x0, base, sw, sh = 1.9, 11.4, 4.05, 1.85
    out = []
    for i, (sl, mode, note) in enumerate(steps):
        x = x0 + i * sw
        h = sh * (i + 1) * 0.52
        y = base - h
        out.append(_rect(x, y, sw - 0.35, h, "stepTop" if i >= 4 else "step"))
        out.append(_label(tbox, x + 0.16, y - 0.92, sw, 0.9, "StepSl", [sl]))
        out.append(_label(tbox, x + 0.16, base + 0.10, sw, 0.8, "StepMode", [mode]))
        if note:
            out.append(_label(tbox, x + 0.16, base + 0.76, sw, 0.8, "StepNote", [note]))
    out.append(_label(tbox, x0, base + 1.44, 24.5, 1.2, "StepFoot",
                      ["Climbs on consecutive clean decodes, not only on an SNR estimate \u2014 and never "
                       "demotes below a level that just decoded.",
                       "A decode is an observation; the SNR is a model. The observation wins. "
                       "(Families shown, not all 14 rungs.)"]))
    return "".join(out)


DIAGRAM_STYLES = f'''
<style:style style:name="Lede" style:family="paragraph"><style:text-properties
 fo:font-size="14pt" fo:font-style="italic" fo:color="#5d6b78"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="tierTop" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{WARM}" draw:stroke="none"/></style:style>
<style:style style:name="tierMid" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{ACCENT}" draw:stroke="none"/></style:style>
<style:style style:name="tierBase" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{INK}" draw:stroke="none"/></style:style>
<style:style style:name="barTrack" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#e8eef4" draw:stroke="none"/></style:style>
<style:style style:name="barGood" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{ACCENT}" draw:stroke="none"/></style:style>
<style:style style:name="barBad" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#a33b3b" draw:stroke="none"/></style:style>
<style:style style:name="blkOwn" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{INK}" draw:stroke="none"/></style:style>
<style:style style:name="blkPlug" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{ACCENT}" draw:stroke="none"/></style:style>
<style:style style:name="blkExt" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="#7e8b96" draw:stroke="none"/></style:style>
<style:style style:name="step" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{INK}" draw:stroke="none"/></style:style>
<style:style style:name="stepTop" style:family="graphic"><style:graphic-properties
 draw:fill="solid" draw:fill-color="{ACCENT}" draw:stroke="none"/></style:style>
<style:style style:name="TierName" style:family="paragraph"><style:text-properties
 fo:font-size="15pt" fo:font-weight="bold" fo:color="#ffffff"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="TierNote" style:family="paragraph"><style:text-properties
 fo:font-size="12pt" fo:color="#dbe7f0" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="TierCannot" style:family="paragraph"><style:text-properties
 fo:font-size="12pt" fo:color="#5d6b78" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="TierAxis" style:family="paragraph"><style:text-properties
 fo:font-size="11.5pt" fo:color="#7e8b96" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BarName" style:family="paragraph"><style:text-properties
 fo:font-size="15pt" fo:font-weight="bold" fo:color="#1c1c1c"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BarVal" style:family="paragraph"><style:text-properties
 fo:font-size="16pt" fo:font-weight="bold" fo:color="{INK}"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BarNote" style:family="paragraph"><style:text-properties
 fo:font-size="11.5pt" fo:color="#5d6b78" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BarFoot" style:family="paragraph"><style:text-properties
 fo:font-size="12pt" fo:color="#5d6b78" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BlkName" style:family="paragraph"><style:text-properties
 fo:font-size="15pt" fo:font-weight="bold" fo:color="#ffffff"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BlkNote" style:family="paragraph"><style:text-properties
 fo:font-size="12.5pt" fo:color="#dbe7f0" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="BlkKey" style:family="paragraph"><style:text-properties
 fo:font-size="11.5pt" fo:color="#5d6b78" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="StepSl" style:family="paragraph"><style:text-properties
 fo:font-size="12pt" fo:font-weight="bold" fo:color="{ACCENT}"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="StepMode" style:family="paragraph"><style:text-properties
 fo:font-size="13pt" fo:font-weight="bold" fo:color="#1c1c1c"
 style:font-name="Liberation Sans"/></style:style>
<style:style style:name="StepNote" style:family="paragraph"><style:text-properties
 fo:font-size="11pt" fo:color="#5d6b78" style:font-name="Liberation Sans"/></style:style>
<style:style style:name="StepFoot" style:family="paragraph">
 <style:paragraph-properties fo:line-height="135%"/><style:text-properties
 fo:font-size="12.5pt" fo:color="#5d6b78" style:font-name="Liberation Sans"/></style:style>
'''
