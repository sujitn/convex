"""Full original Money units on retained receipt provenance; no numeric changes.

Both earlier whole financial artifacts and failed trials remain. Add the required
original currency unit to each expected full cash receipt event, exactly as the
already complete request/native wire specifies. No amounts, values or bounds are
changed and no candidate financial output is used.
"""
import argparse,hashlib,json
from pathlib import Path
from cohort_pnl_wire import units
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
SOURCE=FOLDER/'residual-pnl-original-events.json';TARGET=SOURCE.with_name('residual-pnl-original-units.json')
def generate():
    data=json.loads(SOURCE.read_bytes());units(data)
    from cohort_pnl_states import expand
    from cohort_pnl_reference import sets
    for case in data['cases']:
        if case['expected_error'] is not None:continue
        points=sets(expand(case['request']))
        for j,trade in enumerate(case['expected']['trades']):
            for i,mark in enumerate(trade['marks']):
                for cash in mark['claims']:
                    if cash.get('residual') is None:continue
                    kind='recovery' if cash['key']['kind']=='recovery' else 'contractual_cash'
                    cash['residual']['proposal']=next(s['event'] for s in points[i]['holdings'][j]['lifecycle']['events']
                        if s['event']['action']['kind']==kind and s['event']['effective_date']==cash['key']['effective_date'])
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    raw=(json.dumps(generate(),indent=2)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Exact original full receipt units changed')
    print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
