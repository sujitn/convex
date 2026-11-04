#!/usr/bin/env python3
"""Independent Decimal80 principal recovery and separately earned bond cash."""
import argparse
import copy
from decimal import Decimal, localcontext
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
import core_bond_workflow as bond
import core_lifecycle_workflow as life
import core_snapshot_workflow as core
import lifecycle_risk

PATH = ROOT / 'tests/fixtures/library/bond-events-contract.json'


def generate():
    capture, _, factors = core.prepare()
    curves = copy.deepcopy(capture['curves'])
    requests, expected = {}, {}
    with localcontext() as context:
        context.prec = 80
        df = (-Decimal('0.03') * 50).exp()
        for row in curves:
            if row['curve_id'] == 'D.OIS':
                row['recipe']['source']['Nodes'][-1]['quote']['value'] = float(df)
        bump, time = Decimal('0.0001'), Decimal(14) / 365
        # 183 actual days at 3.5%, original 5000 face, signed -1200 face.
        coupon = Decimal(-1200) * Decimal('0.035') * 183 / 360
        for name, principal, earned_principal in [('original', '-1200', '0'), ('amortizing', '-720', '-480')]:
            value = bond.definition(capture, factors, amortizing=name == 'amortizing')
            config = dict(pnl='pnl', risks={c['request']['operation']['input']['identity']['id']: key
                for key, c in value['calculations'].items() if key.startswith('risk.')})
            value = bond.append(value, [], 'core.bond.control.cancel', cancel=True)
            event = bond.default()
            recovery = bond.recovery()
            if name == 'amortizing':
                event['event']['expected_revision'] = 2
                recovery['event']['expected_revision'] = 3
            value = bond.append(value, [event], 'core.bond.control.default')
            value = bond.resolve(value, recovery)
            value = lifecycle_risk.bind_requests(value, config, [bond.TRADE])
            saved = dict(id='core.bond.control.native', content=dict(definition=dict(definition=dict(definition=value)),
                curves={'market.' + str(i): [r for r in curves if r['version'] == i + 1] for i in range(2)}, fixings=capture['fixings']))
            requests[name] = life.native_request(saved, 'risk.0')
            expected[name] = {}
            for fraction in ['0', '0.35', '1']:
                claim = Decimal(principal) * Decimal(fraction)
                earned = coupon + Decimal(earned_principal)
                samples = [earned + claim * (-(Decimal('0.03') + delta) * time).exp()
                    for delta in [Decimal(0), bump, -bump, bump / 2, -bump / 2]]
                prices = dict(zip(['base_pv', 'plus_pv', 'minus_pv', 'half_plus_pv', 'half_minus_pv'], map(str, samples)))
                prices.update(value=str((samples[1] - samples[2]) / (2 * bump) * bump),
                              half_value=str((samples[3] - samples[4]) / bump * bump))
                expected[name][fraction] = dict(recovery_cash=str(claim), earned_coupon=str(coupon),
                    earned_principal=earned_principal, prices=prices)
    return dict(schema_version=1, provenance='Synthetic independent Decimal80 principal-only bond default control; no market/model certification',
        units=dict(cash='signed USD for actual -1200 face holding; original face5000',
            recovery='0/35/100 percent of actual outstanding principal, not CDS loss-given-default',
            amortization='5000 to3000 at default date, processed step/principal/coupon before default; -480 earned principal retained',
            earned_coupon='183Actual360days at3.5percent on original face; -21.35USD; separate due claim',
            discount='14Actual365Fixeddays at3percent continuous; due earned cash nominal', risk='USD per1bp, one complete native state copy'),
        absolute_bound='0.0000001', relative_bound='0', expected=expected, requests=requests)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    raw = (json.dumps(generate(), sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    if args.mode == 'write':
        if PATH.exists():
            raise RuntimeError('Reference exists; never silently refresh financial expectations')
        PATH.write_bytes(raw)
    elif PATH.read_bytes() != raw:
        raise RuntimeError('Declared bond reference bytes changed')
    print(json.dumps(dict(status='passed', mode=args.mode, reference=str(PATH.relative_to(ROOT)))))


if __name__ == '__main__':
    main()
