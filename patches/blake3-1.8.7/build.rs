use std::env;

fn defined(var: &str) -> bool {
    println!("cargo::rerun-if-env-changed={var}");
    env::var_os(var).is_some()
}

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    assert!(
        defined("CARGO_FEATURE_PURE"),
        "The OrderHub BLAKE3 patch requires the \"pure\" feature; native backends are retired."
    );
    assert!(
        !defined("CARGO_FEATURE_NEON"),
        "The OrderHub BLAKE3 patch does not support the native \"neon\" feature."
    );

    // Register the upstream cfg names still referenced by the unchanged Rust sources,
    // including the native cfgs that this pure-only build never enables.
    for cfg_name in [
        "blake3_sse2_ffi",
        "blake3_sse2_rust",
        "blake3_sse41_ffi",
        "blake3_sse41_rust",
        "blake3_avx2_ffi",
        "blake3_avx2_rust",
        "blake3_avx512_ffi",
        "blake3_neon",
        "blake3_wasm32_simd",
    ] {
        println!("cargo::rustc-check-cfg=cfg({cfg_name}, values(none()))");
    }

    let target_arch =
        env::var("CARGO_CFG_TARGET_ARCH").expect("Cargo must set the target architecture");
    if target_arch == "x86" || target_arch == "x86_64" {
        println!("cargo::rustc-cfg=blake3_sse2_rust");
        println!("cargo::rustc-cfg=blake3_sse41_rust");
        println!("cargo::rustc-cfg=blake3_avx2_rust");
    }
    if target_arch == "wasm32" && defined("CARGO_FEATURE_WASM32_SIMD") {
        println!("cargo::rustc-cfg=blake3_wasm32_simd");
    }
}
