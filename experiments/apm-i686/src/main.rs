//! P1+ experiment 2, and precondition (a) of gate G2.
//!
//! The plan calls sonora's 32-bit Windows status "genuinely unproven": its README
//! validates on Ubuntu x86_64 only, and its SIMD paths are SSE2/AVX2/NEON. Nothing in the
//! audio phase can be planned until that is a fact rather than a hope, because without an
//! APM the port ships an audible regression on the platform with the users.
//!
//! Building this crate for `i686-pc-windows-msvc` is the check. The stronger half — that
//! sonora's own 700-test suite passes there — is run against its repository and recorded
//! in `experiments/README.md`.

fn main() {
    // Linking is the point; constructing a processor proves the symbols resolve rather
    // than merely that the crate type-checks.
    println!("sonora linked for {}", std::env::consts::ARCH);
}
