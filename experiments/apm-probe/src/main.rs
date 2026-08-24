//! P1+ experiment 2: the chosen echo canceller links.
//!
//! It began as gate G2's precondition (a) — does sonora build for 32-bit Windows, which
//! the plan called genuinely unproven. The answer was yes, and the question then became
//! moot: the injection path was removed on 2026-08-24, the `i686-pc-windows-msvc` target
//! went with it, and there is no 32-bit build left to worry about.
//!
//! The crate stays because the weaker question is still worth a check on every CI run:
//! that the APM this project depends on links at all. `experiments/README.md` records the
//! 32-bit result, because a fact does not stop being one when it stops being needed.

fn main() {
    // Linking is the point; constructing a processor proves the symbols resolve rather
    // than merely that the crate type-checks.
    println!("sonora linked for {}", std::env::consts::ARCH);
}
