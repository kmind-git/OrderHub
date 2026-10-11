# P8 provider assessment

Assessment date: 2026-10-11. Selected implementation: rustls 0.23.45,
rustls-graviola 0.4.0, Graviola 0.4.1, on this Windows x64 host.

The [Graviola maintainer's README](https://github.com/ctz/graviola) describes a recent project
and specific CPU requirements. It uses Rust and registered inline assembly. This checkout
does not claim independent cryptographic certification or validation on another CPU/platform.
The supported profile selects X25519/P256/P384 and excludes hybrid ML-KEM. Actual CPU checks
run before installing the shared provider; another installed provider is a hard error.

The [rustls advisory RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285.html)
requires the fixed 0.23.45 line. The locked rustls-webpki is 0.103.15, above the repaired
versions stated in [RUSTSEC-2026-0104](https://rustsec.org/advisories/RUSTSEC-2026-0104.html),
[RUSTSEC-2026-0098](https://rustsec.org/advisories/RUSTSEC-2026-0098.html) and
[RUSTSEC-2026-0099](https://rustsec.org/advisories/RUSTSEC-2026-0099.html).
This targeted review is not a successful full cargo-audit run; the executable is unavailable
on this host. Repository security policy validation is tracked separately.

Local TLS validation covers TLS 1.2/1.3, ECDSA/RSA identities, explicit roots, wrong hostname,
untrusted/expired identities, conflicting provider, missing CPU capabilities and downgrade
rejection. Real node/gateway authentication and authorization continue through the original
bounded core-thread service. Database protocol listeners prove TLS/configuration behavior only.

Live activation requires reviewing the deployed certificate authority, client identity,
revocation/rotation policy, CPU/platform and maintenance updates. New features, provider versions
or another platform invalidate the affected evidence. Real PostgreSQL/Redis authentication,
transactions, reconnection and projection faults remain deferred and disabled at node startup.

Commands, exact source hashes, actual binaries, link inputs and runtime observations are
recorded beside this assessment. Acceptance depends on those results, not this design review.
