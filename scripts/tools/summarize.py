#!/usr/bin/env python3
"""Summarize repeated benchmark runs: mean and spread per job, store versus raw.

Usage: scripts/tools/summarize.py RESULTS_DIR [--since YYYYmmddTHHMMSSZ]

RESULTS_DIR is .lab/results. Run directories are named raw-<stamp> (fio JSON) or
store-<stamp> (flaminio-bench JSON). Only runs at or after --since are included.
"""
import json
import statistics
import sys
from pathlib import Path


def fio_metrics(doc):
    job = doc["jobs"][0]
    d = job["read"] if job["read"]["io_bytes"] > 0 else job["write"]
    pct = d["clat_ns"].get("percentile", {})
    return {
        "iops": d["iops"],
        "mib_s": d["bw_bytes"] / 2**20,
        "p50_us": pct.get("50.000000", 0) / 1000,
        "p99_us": pct.get("99.000000", 0) / 1000,
    }


def store_metrics(doc):
    return {"iops": doc["iops"], "mib_s": doc["mib_s"], "p50_us": doc["lat_us"]["p50"], "p99_us": doc["lat_us"]["p99"]}


def collect(vm_dir, kind, since):
    runs = {}
    for run in sorted(vm_dir.glob(f"{kind}-*")):
        if run.name.split("-", 1)[1] < since:
            continue
        for f in run.glob("*.json"):
            if f.stem == "precondition":
                continue
            m = (fio_metrics if kind == "raw" else store_metrics)(json.loads(f.read_text()))
            runs.setdefault(f.stem, []).append(m)
    return runs


def fmt(values, key):
    xs = [v[key] for v in values]
    mean = statistics.fmean(xs)
    spread = (max(xs) - min(xs)) / mean * 100 if mean else 0
    return mean, spread


def main():
    root = Path(sys.argv[1])
    since = sys.argv[sys.argv.index("--since") + 1] if "--since" in sys.argv else ""
    # Store job names map to the raw job measuring the same thing.
    pairs = [
        ("randread-4k-b1", "randread-4k-qd1", "4K rand read, depth 1"),
        ("randread-4k-b32", "randread-4k-b32", "4K rand read, 32 (closed batches)"),
        ("randread-4k-b32", "randread-4k-qd32", "4K rand read, 32 vs fio continuous"),
        ("read-1m-b8", "read-1m-b8", "1M seq read, 8 (closed batches)"),
        ("read-1m-b8", "read-1m-qd8", "1M seq read, 8 vs fio continuous"),
    ]
    for vm_dir in sorted(p for p in root.iterdir() if p.is_dir()):
        store, raw = collect(vm_dir, "store", since), collect(vm_dir, "raw", since)
        print(f"\n{vm_dir.name}")
        print(f"  {'comparison':<36} {'store':>16} {'raw':>16} {'store/raw':>9}  {'store p50/p99 us':>18}  runs")
        for s_job, r_job, label in pairs:
            if s_job not in store or r_job not in raw:
                continue
            key = "mib_s" if "1m" in s_job else "iops"
            unit = "MiB/s" if key == "mib_s" else "IOPS"
            sm, ss = fmt(store[s_job], key)
            rm, rs = fmt(raw[r_job], key)
            p50 = statistics.fmean(v["p50_us"] for v in store[s_job])
            p99 = statistics.fmean(v["p99_us"] for v in store[s_job])
            print(
                f"  {label:<36} {sm:>9.0f} ±{ss / 2:>3.0f}% {rm:>9.0f} ±{rs / 2:>3.0f}% {sm / rm * 100:>8.0f}%"
                f"  {p50:>8.0f} / {p99:<7.0f}  {len(store[s_job])}/{len(raw[r_job])} ({unit})"
            )


if __name__ == "__main__":
    main()
