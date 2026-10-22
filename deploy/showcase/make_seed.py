#!/usr/bin/env python3
"""Regenerate seed.json for the showcase container (stdlib only, deterministic).

Sources, nothing invented:
  * references: the fictional bond + issuer/calendar/convention/identifier
    command in crates/convex-ext-file/tests/fixtures/instrument-master.json
  * curves: version-1 curve recipes from tools/core_snapshot_workflow.prepare()
    (synthetic, quality "Demo")
  * pricing_configs: one USD configuration naming those curves' roles
    (discount D.OIS, swap benchmark P.SOFR), recorded with the curves
Usage: python deploy/showcase/make_seed.py
"""
import json
import sys
from datetime import datetime
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
recorded_at = curves[0]['recorded_at']
recorded_ms = int(datetime.fromisoformat(recorded_at.replace('Z', '+00:00')).timestamp() * 1000)
roles = {c['curve_id']: c['recipe']['role']['kind'] for c in curves}
assert roles['D.OIS'] == 'Discount' and roles['P.SOFR'] == 'Projection', roles
pricing = dict(
    config_id='SHOWCASE-USD',
    description='Showcase: USD bonds use the demo D.OIS discount and P.SOFR swap curves',
    applies_to=dict(currency='USD'),
    # Configuration validation admits only a mid market-quote model.
    pricing_spec=dict(MarketQuote=dict(side='Mid')),
    bid_ask_spread=None,
    analytics_curves=dict(
        discount_curve='D.OIS', government_curve=None, swap_curve='P.SOFR', frn_reference_curve=None
    ),
    priority=None,
    active=True,
    version=1,
    created_at=recorded_ms,
    updated_at=recorded_ms,
)
seed = dict(
    schema_version=1,
    references=[dict(recorded_at=recorded_at, command=master['command'])],
    curves=curves,
    fixings=[],
    pricing_configs=[pricing],
)
out = Path(__file__).with_name('seed.json')
out.write_text(json.dumps(seed, indent=1) + '\n', newline='\n')
print(f'wrote {out} ({out.stat().st_size} bytes)')
