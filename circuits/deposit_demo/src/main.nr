//! No-input proving harness for the deposit logic (see transfer_demo for rationale).
use notes::deposit::demo_witness;

fn main() {
    demo_witness().run();
}
