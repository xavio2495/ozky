//! No-input proving harness for the transfer logic. Builds a fully valid transfer
//! witness internally (`notes::transfer::demo_witness`) and runs the same constraints
//! as the real `transfer` circuit. With no external inputs it executes from an empty
//! Prover.toml, so `nargo execute` + `bb prove`/`verify` produce a real UltraHonk
//! proof of the transfer constraint system. (The real VK comes from `transfer`; this
//! is a proving smoke test of the logic, not the production circuit.)
use notes::transfer::demo_witness;

fn main() {
    demo_witness().run();
}
