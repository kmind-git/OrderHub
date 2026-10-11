# OrderHub Parquet reader patch

Derived from the published Apache Arrow Rust Parquet 59.3.0 crate. Upstream crate SHA-256: `ff322f54b1a0f9288e614ed1f2d329b380af5476420db19f46ffb865e1163d73`. Copyright, Apache-2.0 license, NOTICE, and original manifest are retained.

Only the Zstandard codec and local dependency manifest change. The optional offline historical converter needs decompression; it does not write Zstandard. Feature `zstd` uses pure Rust `ruzstd = 0.9.0` with checksum verification instead of `zstd-sys`. Encoding returns an explicit unsupported error, preserving output. Pages and frame windows are limited to 64 MiB; concatenated frames are decoded fully, malformed or truncated frames and checksum/size mismatches fail without altering output.

This is a reader-oriented local derivative, not a drop-in upstream Zstandard writer. Upstream writer tests are retained as provenance and are not claimed to pass. OrderHub regression tests cover existing 64/128-bit compressed Parquet fixtures and error/append behavior. Other optional codecs and encryption are outside the supported feature contract.

Primary API source: [ruzstd StreamingDecoder](https://docs.rs/ruzstd/0.9.0/ruzstd/decoding/struct.StreamingDecoder.html).
