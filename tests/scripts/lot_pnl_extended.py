"""Retained original books plus independent future earned coupon/principal.

The original unavailable split date violates knowledge/date admission before
product support can be considered: its declared type is InvalidInput. Only that
error classification changes; all original five full financial books are exact.
Add a full original future amortization profile and its subsequent time/earned
cash point, conserving owned/excluded parent/successor cash. No native values.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import lot_pnl_reference as original
import cohort_pnl_reference as r
import cohort_pnl_states as states
import cohort_pnl_contracts as fixed
import cohort_pnl_cash_dates as dates
import cohort_pnl_calendars as calendars
import residual_pnl_reference as residual
import residual_pnl_canonical as canonical
from cohort_pnl_wire import units
SOURCE=original.TARGET;TARGET=SOURCE.with_name('lot-pnl-extended.json');clone=copy.deepcopy

def future_case():
    previous=original.amend
    def amended(info,label):
        info=previous(info,label);h=info['holdings'][0];t=h['product']['product']['bond']['terms'];rows=[];at='2027-04-07'
        for row in t['profiles']['notional']:
            if row['start']<at:rows.append(dict(row,end=min(at,row['end'])))
            if row['end']>at:rows.append(dict(row,start=max(at,row['start']),value=str(D(row['value'])*D('.75'))))
        t['profiles']['notional']=rows;h['lifecycle']['events'][-1]['event']['action']['terms']['terms']=clone(t)
        return info
    original.amend=amended
    try:case=original.full_case('lot.future.principal_coupon','cohort.full.ctd_ties',single=True)
    finally:original.amend=previous
    spec=case['request']['operation']['input'];book=states.expand(case['request']);owners=clone(spec['cohorts']);splits=clone(spec['lot_splits'])
    opening=r.opening_from(case,book);after=r.c.repoint(book['steps'][-1]['after'],'lot.future.earned')
    r.c.set_contexts(after,'2027-04-07')
    for h in after['holdings']:
        r.c.append_event(h,'2027-04-07','contract_step',known=after['point']['known_at'])
        r.c.append_event(h,'2027-04-07','contractual_cash',known=after['point']['known_at'])
    original.append(book,after,'time')
    for split in splits:split['applied_points'].append(len(book['steps']))
    for j,owner in enumerate(owners):
        for cash in calendars.inferred(after,j,opening,owner)[0]:
            key=r.c.key(cash['kind'],cash['effective'],cash['ordinal'])
            if not any(e['key']==key for e in owner['entitlements']):
                owner['entitlements'].append(dict(key=key,entitled=j!=0,source=r.c.pin(owner['trade_id']+'.FUTURE.LEGAL.'+cash['kind'])))
    previous_acquired=r.acquired
    def acquired(info,owner,physical):
        if owner['acquisition']['kind']!='lot_split':return previous_acquired(info,owner,physical)
        i=next(i for i,s in enumerate(r.sets(book)) if s['point']['snapshot_id']==info['point']['snapshot_id'])
        return any(s['id']==owner['acquisition']['split_id'] and i in s['applied_points'] for s in splits)
    previous=(r.native,r.inferred,r.c.de.cash,r.acquired)
    r.native=fixed.native;r.inferred=calendars.inferred;r.c.de.cash=dates.cash;r.acquired=acquired
    try:
        expected=r.financial(book,owners,[],opening);template=dict(request=residual.reencode(book,owners,[]),expected=expected)
        case=residual.derive(template,book,owners,[],list(range(len(r.sets(book)))))
    finally:r.native,r.inferred,r.c.de.cash,r.acquired=previous
    case['id']='lot.future.principal_coupon';case['request']['operation']['input']['lot_splits']=splits
    units(case);canonical.normalize(case);return case

def generate():
    data=json.loads(SOURCE.read_bytes())
    next(c for c in data['cases'] if c['id']=='lot.invalid.unprocessed_date')['expected_error']='invalid_input'
    with localcontext() as ctx:
        ctx.prec=60;data['cases'].append(future_case())
    case=clone(data['cases'][-1]);case.update(id='lot.invalid.parent_future_cash',expected_error='invalid_input');case.pop('expected')
    next(e for e in case['request']['operation']['input']['cohorts'][0]['entitlements'] if e['key']['effective_date']=='2027-04-07')['entitled']=True
    data['cases'].append(case)
    case=clone(data['cases'][0]);case.update(id='lot.invalid.amended_security_revision',expected_error='invalid_input');case.pop('expected')
    case['request']['operation']['input']['book']['steps'][1]['after']['point']['positions'][0]['identity']['instrument']['version']=1
    data['cases'].append(case)
    for c in data['cases']:c['input_nodes']=original.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original forward financial controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
