# Local Criterion patch

Source: crates.io `criterion` 0.7.0, upstream Criterion.rs, Apache-2.0 OR MIT.
The original licenses, notices and package manifest are retained.
The packaged upstream standalone lock is archived in the P8 baseline, rather than
being a second supported project entry. This library resolves through the root
workspace lock; its upstream CI, examples and self benchmarks are source provenance,
not supported OrderHub commands. It is excluded from workspace membership.

The workspace selects only `cargo_bench_support`, with default features disabled.
This version has no C build dependency. CodSpeed and Iai are retired because their
supported paths compile C hooks or execute Valgrind.

Criterion normally probes and may invoke the external C program gnuplot even
without optional features. This patch makes `gnuplot_version` return an explicit
disabled error without spawning a process. Default reporting remains textual/JSON;
explicit Gnuplot selection fails through the original API validation. Calculations,
sampling and benchmark assertions are unchanged. Optional profiling tools and
external plotting are outside the supported no-C benchmark entry.

Four unused imports and one redundant pair of parentheses in the retained plotting
sources are removed to satisfy the pinned toolchain's warning gate. No plotting
calculation changes.

Validate with the retained benchmark smoke tests, workspace all-target checks,
compiler sentinel, link trace and source review in the P8 acceptance record.
Future upgrades must reassess build dependencies and external program invocation;
do not restore the upstream probe without revising the supported scope.
