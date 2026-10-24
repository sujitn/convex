"""Full original delayed-payment accrued and actual/model CTD comparison.

The original independent dated basket owner prices all entitled cash correctly,
but its accrued helper omitted fully earned coupons between accrual end and the
adjusted payment date. Sum those unpaid periods explicitly, including the actual
Dec1 holiday, and recompute the complete three new physical books at unchanged
bounds. Original native contracts/pricing and all failed financial trials remain.
Add exact independent last-model CTD versus actually delivered security checks.
No native financial output is read or fitted.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as s
import cohort_pnl_contracts as fixed
import cohort_pnl_bindings as claims
SOURCE=r.TARGET.with_name('cohort-pnl-revisions.json')
TARGET=SOURCE.with_name('cohort-pnl-cash-dates.json')
original=r.c.de.cash
def cash(member,curve,delta=D(0),node=None):
    value=original(member,curve,delta,node)
    face=D(member['bond']['terms']['face_value'])
    extra=sum((x['currency_amount'] for x in value['coupons']
        if x['end']<=r.c.de.SETTLEMENT<x['payment']),D(0))/face*100
    value['accrued']+=extra
    value['clean']=value['dirty']-value['accrued']
    return value
def comparisons(case):
    answer=[];spec=case['request']['operation']['input'];trades=case['expected']['trades']
    for link in spec['physical_deliveries']:
        future=next(t for t in trades if t['id']==link['future'])
        asset=next(t for t in trades if t['id']==link['asset'])
        for i,mark in enumerate(asset['marks']):
            if mark['physical'] is None:continue
            model=next((x['native']['full'] for x in reversed(future['marks'][:i+1]) if x['native']['kind']=='delivery'),None)
            ctd={k:model[k] for k in ['selection_ties','fair_ties']} if model else None
            answer.append(dict(point=i,future=link['future'],asset=link['asset'],actual_deliverable=link['instrument'],last_model_ctd=ctd,matches_selection=link['instrument'] in ctd['selection_ties'] if ctd else None,matches_fair=link['instrument'] in ctd['fair_ties'] if ctd else None))
    return answer
def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as context:
        context.prec=60
        prior=(r.c.de.cash,r.native,r.inferred)
        r.c.de.cash=cash;r.native=fixed.native;r.inferred=s.inferred
        try:
            for case in data['cases']:
                if case['expected_error'] is not None:continue
                if case['request']['operation']['input']['physical_deliveries']:
                    book=s.expand(case['request']);spec=case['request']['operation']['input']
                    opening=r.c.ClaimSet([r.rows(t['marks'][0]) for t in case['expected']['trades']]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']]
                    case['expected']=r.financial(book,spec['cohorts'],spec['physical_deliveries'],opening)
                    # Original termination agreements are never model coupons.
                    for trade in case['expected']['trades']:
                        for mark in trade['marks']:
                            assert not any(x['key']['kind']=='termination' for x in mark['claims'])
                    claims.explain(case['expected'],spec,book)
                case['expected']['delivery_comparisons']=comparisons(case)
        finally:r.c.de.cash,r.native,r.inferred=prior
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original delayed cash and CTD controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
