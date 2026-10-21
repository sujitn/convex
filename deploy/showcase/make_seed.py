#!/usr/bin/env python3
"""Regenerate seed.json for the showcase container (stdlib only, deterministic).

Sources, nothing invented:
  * references: the fictional bond + issuer/calendar/convention/identifier
    command in crates/convex-ext-file/tests/fixtures/instrument-master.json
  * curves: version-1 curve recipes from tools/core_snapshot_workflow.prepare()
    (synthetic, quality "Demo")
Usage: python deploy/showcase/make_seed.py
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
import core_snapshot_workflow as core  # noqa: E402
import fixtures  # noqa: E402

# prepare() reads xz-packed library fixtures; a clean checkout has only the archives.
fixtures.materialize()
capture, _plan, _factors = core.prepare()
master = json.loads((ROOT / 'crates/convex-ext-file/tests/fixtures/instrument-master.json').read_text())
curves = [c for c in capture['curves'] if c['version'] == 1]
seed = dict(
    schema_version=1,
    references=[dict(recorded_at=curves[0]['recorded_at'], command=master['command'])],
    curves=curves,
    fixings=[],
)
out = Path(__file__).with_name('seed.json')
out.write_text(json.dumps(seed, indent=1) + '\n', newline='\n')
print(f'wrote {out} ({out.stat().st_size} bytes)')
