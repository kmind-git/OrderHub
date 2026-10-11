# Local OrderHub benchmarking

The retained Rust benches use Criterion 0.7.0, with only `cargo_bench_support` enabled.
The local patch disables external gnuplot probing and execution. CodSpeed, Iai/Valgrind,
Python benchmarks and upstream performance workflows are retired. The previous policy is
preserved in the P8 baseline audit; upstream source attribution remains in each bench.

Run a retained target with the pinned stable toolchain, for example:

```bash
cargo bench --locked -p nautilus-model --bench fixed_precision_criterion
cargo test --locked --profile nextest -p nautilus-core --bench correctness -- --test
```

Historical `_iai` filenames remain for command continuity. Their harness now measures elapsed
time, including the original calculation and correctness assertions. It does not produce
instruction counts. Smoke mode executes assertions once and proves execution only; it is not
capacity or performance evidence.

Compare performance using the same source inputs, toolchain, precision, profile, workload and
machine. Record CPU, operating system, timing controls, executable hash and raw output. Use
domain arithmetic and assert exact semantic results before measuring. Do not infer OMS capacity,
RPO, RTO or live trading readiness from microbenchmarks.

External profilers and plotting tools are separate optional activities outside the supported
no-C benchmark entry. New dependencies must pass `orderhub-check dependency-features` and the
build/link/runtime review in [the no-C plan](orderhub-tls-stack-removal.md).
