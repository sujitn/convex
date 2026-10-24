"""Separate notice-sign admission and additional full native-boundary controls.

The original independent writer, answers, policies and raw failed requests are
immutable. CalendarDays is a signed offset on the wire: independently declared
notice dates become negative offsets, without changing any financial answer.
Calibrated sigma comes from the original independent optional-model fixture;
its complete dated calibration quotes/settings remain in every new request.
"""
import argparse
from copy import deepcopy
import hashlib
import json
import math
from pathlib import Path

from optional_graph_hedge_reference import financial_price, POLICIES

ROOT = Path(__file__).resolve().parents[2]
FOLDER = ROOT / 'tests/fixtures/library'
SOURCES = ['tests/fixtures/library/optional-graph-hedge-api.json',
           'tests/fixtures/library/optional-graph-hedge-reference.json',
           'tests/fixtures/library/optional-graph-hedge-direct.json',
           'tests/fixtures/library/cases.json',
           'crates/convex-analytics/tests/fixtures/optional-bonds.json']


def generate():
    original = json.loads((ROOT / SOURCES[0]).read_bytes())
    admitted = deepcopy(original)
    for case in admitted['cases']:
        op = case['request']['operation']
        if op['product']['kind'] == 'optional_bond':
            for right in op['product']['input']['bond']['terms']['exercise']:
                right['notice_offset']['CalendarDays'] = -abs(right['notice_offset']['CalendarDays'])
    answers = json.loads((ROOT / SOURCES[1]).read_bytes())['cases']
    base = deepcopy(admitted['cases'][0]['request'])
    controls = []

    def negative(label, request, error, change):
        r = deepcopy(request); change(r)
        controls.append(dict(id='optional_graph.qualification.invalid.' + label,
                             request=r, expected_error=error))

    direct = json.loads((ROOT / SOURCES[2]).read_bytes())['cases']
    scenario = next(c['request'] for c in direct
                    if c['id'] == 'optional_graph.direct.volatility.scenario.1')
    risk = next(c['request'] for c in direct
                if c['id'] == 'optional_graph.direct.volatility.risk.h')
    negative('model_scenario_domain', scenario, 'invalid_input',
             lambda r: r['operation']['action'].update(delta=-.03))
    negative('model_scenario_price_refinement', scenario, 'no_convergence',
             lambda r: r['operation']['input']['settings']['grid'].update(
                 absolute_price_tolerance=1e-16, relative_price_tolerance=0.))
    negative('model_risk_domain', risk, 'invalid_input',
             lambda r: r['operation']['action'].update(bump=.03))
    negative('model_risk_refinement', risk, 'no_convergence',
             lambda r: r['operation']['action']['settings'].update(
                 absolute_first_tolerance=1e-12, relative_first_tolerance=0.,
                 absolute_second_tolerance=1e-12, relative_second_tolerance=0.))
    negative('time_basis', base, 'invalid_input',
             lambda r: r['inputs']['curves'][0].update(time_basis='Act360'))
    negative('shaped_node', base, 'invalid_input',
             lambda r: r['operation']['input']['factors'][0]['key']['coordinate'].update(node=1))

    df = deepcopy(base)
    df['inputs']['curves'][0].update(representation='DiscountFactors', interpolation='LogLinear')
    for n in df['inputs']['curves'][0]['source']['Nodes']:
        n['quote'] = dict(value=math.exp(-.04 * n['pillar']['Tenor']), unit='DiscountFactor')
    # Same complete constant-rate economics, independently derived DF units.
    controls.append(dict(id='optional_graph.qualification.discount_factors', request=df,
                         expected_error=None, financial=deepcopy(answers[0]['expected'])))
    def tail(r):
        r['inputs']['curves'][0]['source']['Nodes'][-1].update(
            pillar=dict(Tenor=.1), quote=dict(value=math.exp(-.04 * .1), unit='DiscountFactor'))
    negative('raw_df_tail', df, 'invalid_input', tail)

    # Complete independently sourced sigma fit, actual full original swaption
    # inputs, original information date and original independent financial price.
    supplied = next(c['request'] for c in json.loads((ROOT / SOURCES[3]).read_bytes())['cases']
                    if c['id'] == 'optional.calibrated_model')
    calibrated = deepcopy(base)
    calibrated['inputs'] = deepcopy(supplied['inputs'])
    op = calibrated['operation']; op['product']['input'] = deepcopy(supplied['operation']['input'])
    op['product']['policies'] = deepcopy(POLICIES)
    key = op['product']['input']['discount']; model = op['product']['input']['model']['key']
    for f in op['input']['factors']:
        definition = f['key']['factor']
        if f['key']['coordinate']['kind'] == 'zero':
            f['key']['coordinate']['curve'] = deepcopy(key)
            definition['target'] = dict(kind='curve', curve_id=key['id'])
        if definition['target']['kind'] == 'model':
            definition['target'].update(model_id=model['id'], model_version=model['version'])
    op['input']['identity']['id'] = 'optional_graph.qualification.calibrated'
    sigma = json.loads((ROOT / SOURCES[4]).read_bytes())['oracle']['parameters']['volatility']
    factors = []
    for kind, f in zip(['discount', 'oas', 'a', 'sigma'], op['input']['factors']):
        h = f['settings']['bump']; values = []
        for move in [0., h, -h, h / 2., -h / 2.]:
            values.append(financial_price(op['product']['input'], calibrated['inputs'],
                discount=.04 + (move if kind == 'discount' else 0.),
                oas=move if kind == 'oas' else 0.,
                a=.03 + (move if kind == 'a' else 0.),
                sigma=sigma + (move if kind == 'sigma' else 0.)))
        prices = [v['pv'] for v in values]; unit = 1. if kind in ['a', 'sigma'] else .0001
        factors.append(dict(kind=kind, prices=prices, valuations=values,
            value=(prices[1] - prices[2]) / (2. * h) * unit,
            half_value=(prices[3] - prices[4]) / h * unit))
    controls.append(dict(id='optional_graph.qualification.calibrated', request=calibrated,
                         expected_error=None, financial=dict(factors=factors)))
    meta = dict(certified=False, source_reads={p: hashlib.sha256((ROOT / p).read_bytes()).hexdigest()
        for p in SOURCES}, qualification='Separate signed notice wire admission and additional exact native/curve/model boundaries; original complete financial answers and bounds retained. No native financial captures or certification.')
    admitted.update(meta)
    return admitted, dict(schema_version=1, **meta, cases=controls)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    for name, value in zip(['optional-graph-hedge-admitted.json', 'optional-graph-hedge-qualification.json'], generate()):
        raw = (json.dumps(value, indent=2) + '\n').encode(); p = FOLDER / name
        if args.write:
            if p.exists(): raise ValueError('Refusing to overwrite separate qualification controls')
            p.write_bytes(raw)
        elif p.read_bytes() != raw: raise ValueError('Separate qualification controls differ: ' + name)
        print(json.dumps(dict(status='passed', path=name, cases=len(value['cases']), certified=False)))
