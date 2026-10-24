"""Declare available baseline proposals and partial ingress for delayed child cash.

The prior new delayed experiment retained the due-date seed's May10 knowledge
clock while declaring a May8 baseline. Keep that entire trial. The newly supplied
earned proposals and initial partial security receipt are available on May8;
actual later payment events remain at their original May10 due date. Bind these
same full event bytes in input and provenance, changing no financial value/bound.
"""
import argparse, hashlib, json
import child_cash_pnl_wire as original
SOURCE = original.TARGET
TARGET = SOURCE.with_name('child-cash-pnl-admitted.json')
def generate():
    data = json.loads(SOURCE.read_bytes())
    for case in data['cases']:
        if case['expected_error'] is not None or not case['id'].startswith('child.') or not case['id'].endswith('.delayed'):
            continue
        book = case['request']['operation']['input']['book']; baseline = book['before']
        date = baseline['point']['valuation_date']; known = baseline['point']['known_at']
        ids = {s['event']['event_id'] for k in baseline['holdings'] for s in book['holdings'][k]['lifecycle']['events']}
        def bind(value):
            if isinstance(value, dict):
                if 'event_id' in value and 'action' in value and value['event_id'] in ids:
                    if value['effective_date'] > date:
                        assert value['action']['kind'] == 'security_receipt'
                        value['effective_date'] = date
                    if value['known_at'] > known:
                        value['known_at'] = known
                for child in value.values():bind(child)
            elif isinstance(value, list):
                for child in value:bind(child)
        bind(case)
    data.update(reference=__doc__, source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest())
    return data
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate();raw=(json.dumps(data,indent=2)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original full baseline availability declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),certified=False)))
