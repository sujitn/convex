"""Separate full recovery endpoint/recipe/multiple-helper controls, no native reads."""
import argparse
from copy import deepcopy as clone
from decimal import Decimal as D
import hashlib
import json
from pathlib import Path
from recovery_hedge_reference import cds_price, financial, MOVES

ROOT = Path(__file__).resolve().parents[2]
SOURCE = 'tests/fixtures/library/recovery-hedge-admitted.json'
REFERENCE = 'tests/fixtures/library/recovery-hedge-reference.json'
SOURCES = [SOURCE, REFERENCE, 'tests/scripts/recovery_hedge_reference.py',
           'tests/scripts/recovery_hedge_boundaries.py']


def build():
    original = json.loads((ROOT/SOURCE).read_bytes())['cases']
    base = clone(next(c for c in original if c['id'] == 'recovery_hedge.book.cds')['request'])
    cases = []; answers = []
    for response in ['frozen_curves', 'rebuild_dependents']:
        for delta in [-.4, .6]:
            op = base['operation']; key = clone(op['input']['factors'][4]['key']); key['response'] = response
            request = clone(base)
            request['operation'] = dict(kind='calibration', input=clone(op['graph']), action=dict(kind='product_scenario', product=clone(op['product']), key=key, delta=delta, position=clone(op['input']['reference_position']), snapshot_id=op['input']['snapshot_id']))
            p = op['product']; holidays = next(c['holidays'] for c in base['inputs']['calendars'] if c['pin'] == p['terms']['premium_calendar'])
            expected = cds_price(p['terms'], p['context'], holidays, D('.032'), D('.02'), D('.4')+D(str(delta)))
            baseline = cds_price(p['terms'], p['context'], holidays, D('.032'), D('.02'), D('.4'))
            cases.append(dict(id='recovery_hedge.scenario.endpoint.'+response+'.'+str(delta), request=request, expected_error=None, expected=expected, baseline=baseline))
    calibrated = clone(next(c['request'] for c in original if c['id'] == 'recovery_hedge.rebuilt.calibrated.cds'))
    for label in ['helper_recovery', 'cash_recovery', 'quote_version', 'full_recovery_fit']:
        r = clone(calibrated); op = r['operation']
        if label == 'helper_recovery': op['graph']['credit'][0]['helpers'][0]['terms']['reference']['recovery_fraction'] = .3
        elif label == 'cash_recovery': op['graph']['cash'][0]['recipe']['basis_model']['recovery'] = .3
        elif label == 'quote_version': op['input']['factors'][0]['key']['coordinate']['curve']['version'] = 'unknown.exact.version'
        else:
            r['operation'] = dict(kind='calibration', input=op['graph'], action=dict(kind='product_scenario', product=op['product'], key=op['input']['factors'][0]['key'], delta=.6, position=op['input']['reference_position'], snapshot_id=op['input']['snapshot_id']))
        cases.append(dict(id='recovery_hedge.invalid.recipe.'+label, request=r, expected_error='missing_input' if label == 'quote_version' else 'invalid_input'))
    source = next(c for c in original if c['id'] == 'recovery_hedge.rebuilt.calibrated.cash')
    r = clone(source['request']); recipe = r['operation']['graph']['credit'][0]
    helper = clone(recipe['helpers'][0]); helper['id'] = 'EXPLICIT.SECOND.FULL.CDS.HELPER'
    helper['terms']['trade_id'] = 'EXPLICIT.SECOND.CDS'; helper['terms']['premium_schedule']['end_date'] = '2028-12-20'; helper['pillar'] = '2028-12-20'
    holidays = next(c['holidays'] for c in r['inputs']['calendars'] if c['pin'] == helper['terms']['premium_calendar'])
    helper['quote']['value'] = float(D(cds_price(helper['terms'], helper['context'], holidays, D('.032'), D('.02'), D('.4'))['par_spread_decimal'])*10000)
    recipe['helpers'].append(helper); recipe['curve']['source']['Helpers'].append(helper['id']); recipe['pillars'] = ['2028-12-20', '2029-12-20']
    id = 'recovery_hedge.rebuilt.multiple_helpers.cash'
    expected = clone(next(c['expected'] for c in json.loads((ROOT/REFERENCE).read_bytes())['cases'] if c['id'] == source['id']))
    # All scenarios fit this SAME complete cash security at its original dirty
    # quote. Its known full-PV target is independent of fitted survival nodes.
    h = r['operation']['graph']['cash'][0]['recipe']['helpers'][0]['input']
    target = D(str(h['quote']['value']))*D(h['terms']['face_value'])/100
    expected['factors'][0]['prices'] = [str(target)]*5
    expected['factors'][0]['value'] = expected['factors'][0]['half_value'] = '0'
    cases.append(dict(id=id, request=r, expected_error=None)); answers.append(dict(id=id, expected=expected))
    provenance = dict(certified=False, source_reads={p: hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES}, qualification='Independent complete CDS endpoint scenarios and three full dependent calibration helpers. Original raw/strict-admission inputs/answers/bounds unchanged; full recovery is admitted with held supplied nodes and rejected for identified CDS quote fitting. No native output reads or human certification.')
    return dict(schema_version=1, **provenance, cases=cases), dict(**provenance, cases=answers)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(); parser.add_argument('--write', action='store_true'); args = parser.parse_args()
    for name, value in zip(['recovery-hedge-boundaries.json', 'recovery-hedge-boundary-reference.json'], build()):
        path = ROOT/'tests/fixtures/library'/name; data = (json.dumps(value, indent=2)+'\n').encode()
        if args.write:
            if path.exists(): raise ValueError('Refusing to overwrite controls')
            path.write_bytes(data)
        elif path.read_bytes() != data: raise ValueError('Separate controls differ: '+name)
        print(json.dumps(dict(status='passed', cases=len(value['cases']), path=name, certified=False)))
