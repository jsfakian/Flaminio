//! Test vectors for the metadata service's transitions (protocol spec §2).
//!
//! Drives [`Metadata`] through random sequences of `Attach` and `ChangeIsr`, valid and not,
//! and prints one JSON object per line: the request and the reply. The Go metadata service
//! replays them (`meta/domain`), so both implementations follow the same table.
//!
//! `cargo run -p flaminio-repl --example meta_vectors > ../meta/testdata/isr-vectors.jsonl`

use std::collections::BTreeSet;
use std::fmt::Write;

use flaminio_repl::meta::{IsrChange, MetaReply, MetaRequest, Metadata, VolumeMeta};
use flaminio_repl::sim::Rng;
use flaminio_repl::types::NodeId;

fn nodes(n: impl IntoIterator<Item = NodeId>) -> String {
    let v: Vec<String> = n.into_iter().map(|n| format!("\"n{n}\"")).collect();
    format!("[{}]", v.join(","))
}

fn view(v: &VolumeMeta) -> String {
    let chunks: Vec<String> = v
        .chunks
        .iter()
        .map(|c| {
            format!(
                "{{\"replicas\":{},\"isr\":{},\"isr_ver\":{}}}",
                nodes(c.replicas.iter().copied()),
                nodes(c.isr.iter().copied()),
                c.isr_ver
            )
        })
        .collect();
    let target = v.target.map(|t| format!("n{t}")).unwrap_or_default();
    format!(
        "{{\"epoch\":{},\"target\":\"{target}\",\"chunks\":[{}]}}",
        v.epoch,
        chunks.join(",")
    )
}

fn main() {
    let mut out = String::new();
    for seed in 0..200u64 {
        let mut r = Rng::new(seed);
        let n_nodes = r.range(3, 5) as NodeId;
        let rf = r.range(1, 3.min(n_nodes as u64)) as usize;
        let min_isr = r.range(1, rf as u64) as usize;
        let chunks = r.range(1, 4);
        let placement: Vec<Vec<NodeId>> = (0..chunks)
            .map(|_| {
                let mut pool: Vec<NodeId> = (0..n_nodes).collect();
                (0..rf)
                    .map(|_| pool.remove((r.next_u64() % pool.len() as u64) as usize))
                    .collect()
            })
            .collect();
        let mut m = Metadata::default();
        m.create_volume(1, min_isr, 4, placement.clone());
        let pl: Vec<String> = placement.iter().map(|p| nodes(p.iter().copied())).collect();
        writeln!(
            out,
            "{{\"op\":\"create\",\"min_isr\":{min_isr},\"placement\":[{}]}}",
            pl.join(",")
        )
        .unwrap();
        for _ in 0..r.range(5, 40) {
            let v = m.volume(1).unwrap().clone();
            let req = if r.pct(20) {
                MetaRequest::Attach {
                    vol: 1,
                    node: r.range(0, n_nodes as u64 - 1) as NodeId,
                }
            } else {
                // Mostly plausible changes, sometimes stale or out of range.
                let epoch = if r.pct(85) {
                    v.epoch
                } else {
                    r.range(0, v.epoch + 1)
                };
                let k = r.range(0, chunks.min(3));
                let mut picked = BTreeSet::new();
                let changes = (0..k)
                    .filter_map(|_| {
                        let chunk = r.range(0, chunks - 1) as u32;
                        if !picked.insert(chunk) && !r.pct(10) {
                            return None;
                        }
                        let c = &v.chunks[chunk as usize];
                        let expected_ver = if r.pct(85) {
                            c.isr_ver
                        } else {
                            r.range(0, c.isr_ver + 1)
                        };
                        let new_isr: BTreeSet<NodeId> = (0..n_nodes)
                            .filter(|n| {
                                let p = if c.replicas.contains(n) { 65 } else { 5 };
                                r.pct(p)
                            })
                            .collect();
                        Some(IsrChange {
                            chunk,
                            expected_ver,
                            new_isr,
                        })
                    })
                    .collect();
                MetaRequest::ChangeIsr {
                    vol: 1,
                    epoch,
                    changes,
                }
            };
            let reply = m.handle(&req);
            let (applied, after) = match &reply {
                MetaReply::Applied { view, .. } => (true, view),
                MetaReply::Rejected { view, .. } => (false, view),
                MetaReply::NotFound { .. } => unreachable!(),
            };
            let line = match &req {
                MetaRequest::Attach { node, .. } => format!(
                    "{{\"op\":\"attach\",\"node\":\"n{node}\",\"applied\":{applied},\"after\":{}}}",
                    view(after)
                ),
                MetaRequest::ChangeIsr { epoch, changes, .. } => {
                    let cs: Vec<String> = changes
                        .iter()
                        .map(|c| {
                            format!(
                                "{{\"chunk\":{},\"expected_ver\":{},\"new_isr\":{}}}",
                                c.chunk,
                                c.expected_ver,
                                nodes(c.new_isr.iter().copied())
                            )
                        })
                        .collect();
                    format!(
                        "{{\"op\":\"change_isr\",\"epoch\":{epoch},\"changes\":[{}],\"applied\":{applied},\"after\":{}}}",
                        cs.join(","),
                        view(after)
                    )
                }
            };
            out.push_str(&line);
            out.push('\n');
        }
    }
    print!("{out}");
}
