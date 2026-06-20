//! No-input proving harness for the withdraw logic (see transfer_demo for rationale).
use notes::withdraw::demo_witness;

fn main() {
    demo_witness().run();
}
