import math,cmath
def est_ppm(seg, sr, f0, block_s=0.5):
    """Block-wise phase, unwrapped, least-squares slope. No wrap up to ~1/(f0*block_s) relative."""
    B=int(sr*block_s); nb=len(seg)//B
    if nb<4: return float('nan')
    ph=[]
    for b in range(nb):
        acc=sum(cmath.exp(-2j*math.pi*f0*(b*B+k)/sr)*seg[b*B+k] for k in range(B))
        ph.append(cmath.phase(acc))
    # unwrap
    up=[ph[0]]
    for k in range(1,nb):
        d=ph[k]-ph[k-1]
        d=(d+math.pi)%(2*math.pi)-math.pi
        up.append(up[-1]+d)
    t=[b*block_s for b in range(nb)]
    tm=sum(t)/nb; pm=sum(up)/nb
    num=sum((t[i]-tm)*(up[i]-pm) for i in range(nb))
    den=sum((t[i]-tm)**2 for i in range(nb))
    df=(num/den)/(2*math.pi)
    return df*1e6/f0

if __name__ == "__main__":
    # Self-test: an estimator that cannot detect a KNOWN offset must not be trusted to report an
    # unknown one. The first version of this code wrapped phase above ~5 ppm and reported a large
    # offset as "+0.1 ppm" -- indistinguishable from a genuinely clean rig.
    import sys
    sr, f0, dur = 48000, 1000.0, 20
    bad = 0
    for true_ppm in (0.0, 0.1, 5.0, 50.0, 200.0, 800.0, -300.0):
        f = f0 * (1 + true_ppm / 1e6)
        seg = [12000 * math.sin(2 * math.pi * f * n / sr) for n in range(sr * dur)]
        m = est_ppm(seg, sr, f0)
        ok = abs(m - true_ppm) < max(0.5, abs(true_ppm) * 0.02)
        bad += not ok
        print(f"  injected {true_ppm:8.1f} ppm -> measured {m:8.1f} ppm  {'ok' if ok else 'BAD'}")
    sys.exit(1 if bad else 0)
