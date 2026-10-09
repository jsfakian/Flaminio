//! Runs the replication simulator over a range of seeds.
//!
//! `cargo run --release -p flaminio-repl --example sim -- [first-seed] [count]`

use std::time::Instant;

fn main() {
    let args: Vec<u64> = std::env::args()
        .skip(1)
        .map(|a| a.parse().expect("seed numbers"))
        .collect();
    let first = args.first().copied().unwrap_or(0);
    let count = args.get(1).copied().unwrap_or(1000);
    let start = Instant::now();
    let mut failed = 0;
    for seed in first..first + count {
        let t = Instant::now();
        match flaminio_repl::sim::run_seed(seed) {
            Ok(r) => {
                if std::env::var_os("VERBOSE").is_some() {
                    eprintln!("seed {seed}: {r:?}");
                }
                if t.elapsed().as_secs() >= 2 {
                    eprintln!("seed {seed}: slow, {:?}, {} events", t.elapsed(), r.events);
                }
            }
            Err(e) => {
                failed += 1;
                println!("seed {seed}: FAILED: {e}");
            }
        }
    }
    println!(
        "{count} seeds from {first}: {} passed, {failed} failed, {:.1?}",
        count - failed,
        start.elapsed()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
