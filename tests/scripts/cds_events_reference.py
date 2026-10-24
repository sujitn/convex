#!/usr/bin/env python3
"""Independent Decimal80 actual CDS recovery/default-accrual claim prices/risk."""
import argparse
import copy
from decimal import Decimal, localcontext
import json
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
import core_cds_workflow as cds
import core_lifecycle_workflow as life
import core_snapshot_workflow as core
import lifecycle_risk
PATH = ROOT / 'tests/fixtures/library/cds-events-contract.json'

def generate():
    capture, _, factors = core.prepare()
    value = cds.definition(capture, factors)
    config = dict(pnl='pnl', risks={c['request']['operation']['input']['identity']['id']: name for name, c in value['calculations'].items() if name.startswith('risk.')})
    value = cds.append(value, [], 'core.cds.control.cancel', cancel=True)
    value = cds.append(value, [cds.default()], 'core.cds.control.default')
    value = cds.resolve(value, cds.recovery())
    value = lifecycle_risk.bind_requests(value, config, [cds.TRADE])
    curves = copy.deepcopy(capture['curves'])
    with localcontext() as context:
        context.prec = 80
        df = (-Decimal('0.03') * 50).exp()
        for row in curves:
            if row['curve_id'] == 'D.OIS':
                row['recipe']['source']['Nodes'][-1]['quote']['value'] = float(df)
        accrual = Decimal(450000) * Decimal('0.01') * Decimal('17.5') / 360
        expected = {}
        bump, unit, time = (Decimal('0.0001'), Decimal('0.0001'), Decimal(14) / 365)
        for recovery in ['0', '0.35', '1']:
            protection = Decimal(-450000) * (1 - Decimal(recovery))
            amount = protection + accrual
            samples = [amount * (-(Decimal('0.03') + delta) * time).exp() for delta in [Decimal(0), bump, -bump, bump / 2, -bump / 2]]
            row = dict(zip(['base_pv', 'plus_pv', 'minus_pv', 'half_plus_pv', 'half_minus_pv'], map(str, samples)))
            row.update(value=str((samples[1] - samples[2]) / (2 * bump) * unit), half_value=str((samples[3] - samples[4]) / bump * unit))
            expected[recovery] = dict(protection_cash=str(protection), default_accrual=str(accrual), prices=row)
    saved = dict(id='core.cds.control.native', content=dict(definition=dict(definition=dict(definition=value)), curves={'market.' + str(i): [r for r in curves if r['version'] == i + 1] for i in range(2)}, fixings=capture['fixings']))
    return dict(schema_version=1, provenance='Synthetic independent Decimal80 contractual actual CDS claim control; not a market/model approval pack', units=dict(cash='signed USD for actual whole holding', notional='-450000USD protection buyer', accrual='17.5actual elapsed days /360;1%coupon; explicitNearestEven2cash precision', discount='14Actual365Fixeddays at3%continuous', risk='USD per1bp; one complete actual state copy'), absolute_bound='0.0000001', relative_bound='0', expected=expected, request=life.native_request(saved, 'risk.5'))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    raw = (json.dumps(generate(), sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    if args.mode == 'write':
        if PATH.exists():
            raise RuntimeError('Reference already exists; never silently refresh it')
        PATH.write_bytes(raw)
    elif PATH.read_bytes() != raw:
        raise RuntimeError('Declared CDS reference bytes changed')
    print(json.dumps(dict(status='passed', mode=args.mode, reference=str(PATH.relative_to(ROOT)))))
if __name__ == '__main__':
    main()
