# GPU LDPC belief-propagation prototype — findings

Status: **prototype / decision = do NOT productise for the real-time HF path.**

A flooding-schedule min-sum belief-propagation LDPC decoder was prototyped on the
GPU (`crates/openpulse-gpu/src/ldpc_bp.rs`) to answer one question: is GPU LDPC
worth wiring into the modem's decode path? Short answer — **no for single-block
real-time decode (≈9× slower than CPU); a modest ~2–3× win only for bulk/batched
decode of ~40+ codewords at once**, which the HF modem never does (it decodes one
LDPC block per frame).

## What was built

- Two WGSL compute kernels mirroring the CPU min-sum in `openpulse-core::ldpc`:
  a variable-node *accumulate* (`total[v] = ch[v] + Σ c2v`) and a check-node
  *min-sum update* (extrinsic sign-product × min-magnitude, snapshotting the old
  messages per check exactly like the CPU `ext` vector).
- Batching: `B` independent codewords share the static Tanner-graph buffers and
  run through the same dispatch grid (`B·n` / `B·m` threads), so the fixed
  per-dispatch overhead amortises across blocks.
- Fixed iteration count (no per-iteration syndrome read-back): all `2·iters + 1`
  dispatches are queued in one command encoder and synced once. Early termination
  would force a GPU→CPU sync every iteration, which defeats the single-submit
  batching advantage — so fixed-iteration is the correct GPU design.

## Correctness

`gpu_ldpc_matches_cpu` (ignored, needs an adapter): the GPU decoder agrees with
the CPU decoder **bit-for-bit on 24/24 converged blocks**. The GPU implements the
same algorithm; it is correct.

## Benchmark (this box's adapter, release build, k=1024 / n=2048, 50 iters)

CPU min-sum is the early-terminating decoder (its real-world behaviour); GPU runs
a fixed 50 iterations.

| blocks | GPU total | GPU per-block | vs CPU (0.56 ms/block) |
|-------:|----------:|--------------:|-----------------------:|
| 1      | 5.07 ms   | 5.07 ms       | **0.11× (9× slower)**  |
| 8      | 6.91 ms   | 0.86 ms       | 0.65× (slower)         |
| 64     | 18.21 ms  | 0.28 ms       | 1.97× faster           |
| 256    | 53.33 ms  | 0.21 ms       | 2.68× faster           |

Break-even is ~40 codewords decoded in one batch. Reproduce with:

```bash
cargo test -p openpulse-gpu --release --test ldpc_bp_bench -- --ignored --nocapture
```

## Why single-block loses

The 2048-variable / 1024-check graph is tiny for a GPU — each of the ~101
dispatches barely occupies the device yet still pays a fixed per-dispatch cost,
and the message buffers must round-trip through GPU memory. The CPU min-sum, with
early termination, finishes a block in ~0.56 ms — faster than the GPU's fixed
50-iteration single-block decode can even be submitted and synced. This matches
the broader finding (see `project_gpu_acceleration` memory): HF's real-time
single-frame model is a poor GPU fit; the GPU only pays off on batch/throughput
work.

## Recommendation

- **Do not** wire GPU LDPC into the engine decode path — it would regress latency
  ~9× on the one-block-per-frame workload, and far worse on a Raspberry Pi (weaker
  GPU, higher dispatch overhead → likely never beneficial).
- The prototype (kernels + benchmark) is kept in `openpulse-gpu` as the measured
  basis for this decision and as a starting point should a *bulk* offline decode
  use case ever appear (e.g. decoding a large stored capture), where ~2–3× is on
  the table at ≥64-block batches.
- The real lever for "use the GPU more" remains **batching** sample/symbol-domain
  work (many frames/FFTs per dispatch), not per-mode or per-block expansion.
