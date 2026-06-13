#!/usr/bin/env bash
# Chirp frequency-response probe for the HARDWARE loopback path (rpi51 -> rpi52).
#
# Plays a linear chirp through the real cable and records it, twice:
#   A (48k): file rate == card rate -> NO ALSA resample. Isolates analog path
#            (cards + cable + ground-loop isolator).
#   B (8k):  via plughw -> 8k<->48k resample both ends. Adds the resampler.
# Comparing A and B separates the analog path from the resampler. See
# docs/dev/virtual-loopback.md. Analysis via scripts/analyze-loopback-response.py.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ISS_SSH="${ISS_SSH:-dc0sk@dc0sk-rpi51}"
IRS_SSH="${IRS_SSH:-dc0sk@dc0sk-rpi52}"
SSH_OPTS="${SSH_OPTS:--o BatchMode=yes -o ConnectTimeout=10}"
CARD="${CARD:-plughw:CARD=Device,DEV=0}"
OUT="${OUT:-/tmp}"

ssh_iss() { ssh ${SSH_OPTS} "${ISS_SSH}" "$@"; }
ssh_irs() { ssh ${SSH_OPTS} "${IRS_SSH}" "$@"; }

echo "== generating chirps on ${ISS_SSH} =="
ssh_iss 'python3 - <<PY
import wave, math, struct
def chirp(path, sr, f0=50, f1=3950, T=4.0, A=0.5, lead=0.5, trail=0.5):
    out = [0]*int(sr*lead)
    n = int(sr*T); k = (f1-f0)/T
    for i in range(n):
        t = i/sr
        out.append(int(A*32767*math.sin(2*math.pi*(f0*t+0.5*k*t*t))))
    out += [0]*int(sr*trail)
    w = wave.open(path,"w"); w.setnchannels(1); w.setsampwidth(2); w.setframerate(sr)
    w.writeframes(b"".join(struct.pack("<h", max(-32768,min(32767,v))) for v in out)); w.close()
chirp("/tmp/chirp_48k.wav", 48000); chirp("/tmp/chirp_8k.wav", 8000)
print("ok")
PY'

# Match the modem capture conditions (AGC off, capture volume max).
_card="${CARD#*CARD=}"; _card="${_card%%,*}"
ssh_irs "amixer -c ${_card} cset name='Auto Gain Control' 0 >/dev/null 2>&1 || true; \
         amixer -c ${_card} cset name='Mic Capture Volume' 35 >/dev/null 2>&1 || true" || true

run() {  # tag rate wav
    local tag=$1 rate=$2 wav=$3
    echo "== test ${tag} (rate=${rate}) =="
    # pkill -x (exact name) so it never matches the remote shell whose cmdline contains 'arecord'.
    ssh_irs "pkill -x arecord 2>/dev/null; sleep 0.2; \
        nohup arecord -D ${CARD} -f S16_LE -r ${rate} -c 1 -d 15 /tmp/rx_${tag}.wav \
        >/tmp/arec_${tag}.log 2>&1 </dev/null & disown; echo rec-started"
    sleep 1
    ssh_iss "aplay -D ${CARD} ${wav} >/dev/null 2>&1; echo played"
    sleep 3
    scp -q ${SSH_OPTS} "${IRS_SSH}:/tmp/rx_${tag}.wav" "${OUT}/rx_${tag}.wav"
    echo "   -> ${OUT}/rx_${tag}.wav"
}
run 48k 48000 /tmp/chirp_48k.wav
run 8k  8000  /tmp/chirp_8k.wav

echo "== analysis =="
python3 "${REPO_ROOT}/scripts/analyze-loopback-response.py" --png "${OUT}/path_response.png" \
    "48k (analog: cards+cable+isolator, no resample)=${OUT}/rx_48k.wav" \
    "8k (modem path incl. ALSA resampler)=${OUT}/rx_8k.wav"
