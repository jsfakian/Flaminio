#!/usr/bin/env python3
"""Run desired-contract audit tests; failures are expected on audited main 8aaa7a3."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("group", choices=("go", "rust"))
args = parser.parse_args()
here = Path(__file__).resolve().parent
repo = here.parents[3]
with tempfile.TemporaryDirectory(prefix="flaminio-audit-repro-") as temporary:
    scratch = Path(temporary)
    if args.group == "go":
        replacements = {}
        for package, source in (
            ("meta/service", "metadata_test.go.txt"),
            ("control/csi", "csi_test.go.txt"),
            ("control/failover", "failover_test.go.txt"),
        ):
            replacements[str(repo / package / "audit_repro_test.go")] = str(here / source)
        overlay = scratch / "overlay.json"
        overlay.write_text(json.dumps({"Replace": replacements}))
        command = ["go", "test", "-overlay", str(overlay), "./meta/service", "./control/csi",
                   "./control/failover", "-run", "^TestAudit", "-count=1", "-v"]
    else:
        (scratch / "src").mkdir()
        dependencies = ["flaminio-format", "flaminio-net", "flaminio-node", "flaminio-store",
                        "flaminio-repl", "flaminio-uring"]
        manifest = '[package]\nname="flaminio-audit-repro"\nversion="0.0.0"\nedition="2024"\n[dependencies]\n'
        for name in dependencies:
            path = repo / "dataplane/crates" / name
            manifest += f"{name} = {{ path = {json.dumps(str(path))} }}\n"
        manifest += 'libc = "0.2"\n'
        (scratch / "Cargo.toml").write_text(manifest)
        # Preserve exact audited dependency versions, including locked yanked crates.
        shutil.copyfile(repo / "dataplane/Cargo.lock", scratch / "Cargo.lock")
        (scratch / "src/lib.rs").write_text(
            (here / "geometry.rs.txt").read_text() + (here / "uring.rs.txt").read_text())
        command = ["cargo", "+1.99.0", "test", "--offline", "--manifest-path",
                   str(scratch / "Cargo.toml"), "--target-dir", str(repo / "dataplane/target")]
    print("Desired-contract tests: failures are expected on main 8aaa7a3.", flush=True)
    raise SystemExit(subprocess.run(command, cwd=repo, check=False).returncode)
