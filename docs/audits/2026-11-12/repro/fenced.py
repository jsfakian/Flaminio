#!/usr/bin/env python3
"""Safe in-memory F09 probe. Exit0 asserts BOTH current framing defects are present."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[4]
source = Path(__file__).with_name('fenced.rs.txt')
with tempfile.TemporaryDirectory(prefix='flaminio-fenced-') as scratch:
    package = Path(scratch)
    (package / 'src').mkdir()
    dependencies = '\n'.join(
        name + ' = { path = ' + json.dumps(str(repo / 'dataplane/crates' / name)) + ' }'
        for name in ('flaminio-repl', 'flaminio-net')
    )
    (package / 'Cargo.toml').write_text(
        '[package]\nname = "flaminio-fenced-repro"\nversion = "0.0.0"\nedition = "2024"\n'
        '[dependencies]\n' + dependencies + '\n'
    )
    (package / 'src/main.rs').write_bytes(source.read_bytes())
    task_env = dict(os.environ)
    task_env.setdefault('CARGO_TARGET_DIR', str(package / 'target'))
    completed = subprocess.run(
        ['cargo', 'run', '--offline', '--quiet', '--manifest-path', str(package / 'Cargo.toml')],
        cwd=repo, env=task_env, check=False,
    )
    raise SystemExit(completed.returncode)
