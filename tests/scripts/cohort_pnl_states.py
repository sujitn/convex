"""Separate original prior-default state and full physical receipt controls.

The initial financial preparation remains immutable. A seasoned actual recovery
claim lot starts after its full original default/auction prefix, so its surviving
model exposure is ended before acquisition; original outstanding cash is retained.
No candidate financial answer is read and no bound is changed.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import causal_pnl_wire as wire
SOURCE=r.TARGET;TARGET=SOURCE.with_name('cohort-pnl-states.json')
original=r.inferred
def inferred(info,j,opening,owner):
    cash,status,remaining,notices=original(info,j,opening,owner)
    prefix=info['holdings'][j]['lifecycle']['events'][:opening._events[j]]
    for step in prefix:
        k=step['event']['action']['kind']
        if k=='credit_default':status='DefaultPending';remaining=D(0);notices=[]
        if k=='recovery':status='DefaultCashPending';remaining=D(0);notices=[]
    if status=='DefaultCashPending' and all(x['amount']==x['recorded'] for x in cash):status='Closed'
    return cash,status,remaining,notices
def expand(req):
    return wire.expand(dict(schema_version=req['schema_version'],inputs=req['inputs'],operation=dict(kind='book_pnl',input=req['operation']['input']['book'])))['operation']['input']
def generate():
    data=json.loads(SOURCE.read_bytes());case=next(x for x in data['cases'] if x['id']=='cohort.credit.acquired_actual_recovery_claim');book=expand(case['request']);spec=case['request']['operation']['input'];opening=r.c.ClaimSet([r.rows(t['marks'][0]) for t in case['expected']['trades']]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']]
    with localcontext() as context:
        context.prec=60;old=r.inferred;r.inferred=inferred
        try:case['expected']=r.financial(book,spec['cohorts'],spec['physical_deliveries'],opening)
        finally:r.inferred=old
    def reject(name,fn,source='cohort.delivery.long_security_then_cash',code='invalid_input'):
        x=copy.deepcopy(next(x for x in data['cases'] if x['id']==source));x.update(id=name,expected_error=code);x.pop('expected');fn(x['request']['operation']['input']);data['cases'].append(x)
    def receipt(x):
        for h in x['book']['holdings']:
            for step in h['lifecycle']['events']:
                if step['event']['action']['kind']=='security_receipt':step['event']['action']['terms']['face']='1'
    def foreign_terms(x):
        for h in x['book']['holdings']:
            if h['id']=='cohort.actual.asset':
                for t in [h['product']['product']['bond']['terms'],h['lifecycle']['instrument']['bond']['terms']]:t['profiles']['coupon'][0]['value']='0.09'
    reject('cohort.invalid.actual_security_receipt_face',receipt)
    reject('cohort.invalid.foreign_complete_asset_terms',foreign_terms)
    def unknown(x):
        for h in x['book']['holdings']:
            h['lifecycle']['events']=[s for s in h['lifecycle']['events'] if s['event']['action']['kind']!='recovery']
    reject('cohort.invalid.original_unknown_recovery',unknown,'cohort.credit.acquired_actual_recovery_claim','missing_input')
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original prior-event financial declaration differs')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
