"""Independent full OIS observation eligibility at original economic dates.

Preserve the upstream failed full financial trial. Receiving observations at a
later knowledge cutoff does not make future economic observations historical at
an earlier valuation date. Recompute the SAME complete original roll using both
date and received-publication eligibility, unchanged schedules/curves/profiles,
independent Decimal pricing, all native fields/claims/CTD/P&L and original bounds.
No candidate output is read and no earlier reference or tolerance is overwritten.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_reconciled as r
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=c.FOLDER/'causal-pnl-contexts.json'
TARGET=SOURCE.with_name('causal-pnl-economic-cutoffs.json')
NAME='causal.full.time_fixing_cash_ctd.actual_cash.complete_publications.settlement_eligible_roll'

def native(info,h):
    p=h['product'].get('product',h['product'])
    if p['kind']!='swap' or p['terms']['trade_id'].startswith('lifecycle.synthetic.'):
        return r.native(info,h)
    t=p['terms'];on=info['point']['valuation_date'];known=info['point']['known_at']
    history={(x['index_id'],c.day(x['observation_date'])):D(x['rate']) for x in info['inputs']['fixings'] if x['observation_date']<=on and x['published_at']<=known and x['known_at']<=known and x['status']=='Final'}
    discount=c.curve_zero(info['inputs'],t['collateral']['discount_curve']['id']);projection=c.curve_zero(info['inputs'],t['floating']['projection_curve']['id'])
    result=c.g.swap_price(t,discount,projection,history)
    anchor=(discount*D((c.day(on)-c.g.REF).days)/365).exp();pv=D(0)
    for leg in ['fixed','floating']:
        rows=[]
        for row in result[leg]:
            if row['payment']<=on:continue
            row=copy.deepcopy(row);row['pv']=D(row['pv'])*anchor;rows.append(row);pv+=row['pv']
        result[leg]=rows
    result['pv']=pv
    return dict(kind='swap',pv=pv,full=result)

def generate():
    data=json.loads(SOURCE.read_bytes());case=next(x for x in data['cases'] if x['id']==NAME)
    req=wire.expand(case['request']);inp=req['operation']['input'];rows=[]
    for trade in case['expected']['trades']:
        rows.append([dict(kind=x['key']['kind'],effective=x['key']['effective_date'],ordinal=x['key']['ordinal'],due=x['due'],amount=D(x['amount']),recorded=D(x['recorded'])) for x in trade['marks'][0]['claims']])
    opening=c.ClaimSet(rows);opening._events=[len(h['lifecycle']['events']) for h in inp['before']['holdings']]
    with localcontext() as context:
        context.prec=60;old=a.native;a.native=native
        try:case['expected']=a.financial(req,opening)
        finally:a.native=old
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Same full original nine-product request with independently eligible OIS observations bounded by BOTH valuation date and knowledge/publication cutoff. All complete original financial fields/cash/CTD/P&L recomputed at unchanged bounds. Prior failed full reference remains upstream. No candidate output used.')
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent full economic-date cutoffs differ')
    data=json.loads(raw);print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
