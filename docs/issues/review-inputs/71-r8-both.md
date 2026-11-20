# Plan review #71 round 8 (incremental), design-code and design-security
R7-1: fixed. No defects found in the diff e94d7af..f68610a.
Checked: node.rs:120 parse and main.rs:13 (mod node ungated; control/ublk gated); node.rs:474 test module calls parse directly; main.rs:35-39 maps Err to exit 1 with the message. "exits 1" is a main-level consequence, but the test asserts the Err string, which is adequate.
