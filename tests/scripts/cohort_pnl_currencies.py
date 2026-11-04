"""Full ten-product native USD/EUR cash and reporting control, before execution.

Reuse all complete original nine-product fixing/time/cash/market and CTD inputs;
add an explicitly supplied full EUR stepped/amortizing bond cohort, separate EUR
collateral curve and direct received EUR-to-USD observation. Full original prices,
cash keys/dates/amounts, CTD and attribution are independently recomputed, at
original bounds. Earlier failures/controls remain immutable. No native output.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as s
import cohort_pnl_contracts as cash
import cohort_pnl_wire as money
import causal_pnl_wire as wire
import causal_pnl_ready as binding
SOURCE=r.TARGET.with_name('cohort-pnl-bindings.json');TARGET=SOURCE.with_name('cohort-pnl-currencies.json')
def generate():
    data=json.loads(SOURCE.read_bytes());case=next(x for x in data['cases'] if x['id']=='cohort.full.nine_product_roll_cash_ctd');book=s.expand(case['request']);book['reporting_currency']='USD';opening=r.c.ClaimSet([r.rows(t['marks'][0]) for t in case['expected']['trades']]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']]
    before=book['before'];p=copy.deepcopy(before['point']['positions'][0]);p['identity'].update(id='cohort.EUR.cash',instrument=r.c.pin('COHORT.EUR.BOND'),economics=r.c.pin('COHORT.EUR.ECONOMICS'),calculation=r.c.pin('COHORT.EUR.CALCULATION'));p['current'].update(currency='EUR',quantity='-700');p['pending_quantity']='50';p['cash_discount']=dict(id='D.COHORT.EUR',version='1');product=copy.deepcopy(before['holdings'][0]['product']);product['product']['bond']['terms']['currency']='EUR';product['product']['method']['curve']=copy.deepcopy(p['cash_discount']);h=dict(id='cohort.EUR.cash',product=product);h,claims=r.c.lifecycle(h,p,before['inputs'],before['point']['valuation_date']);curve=r.c.op.curve(p['cash_discount'],.027,dict(kind='Discount',collateral_currency='EUR'),'2026-10-07');curve['currency']='EUR';fx=copy.deepcopy(before['point']['fx'][0]);fx.update(from_='EUR') if False else None;fx['from']='EUR';fx['to']='USD';fx['rate']='1.10';fx['observation']=r.c.pin('COHORT.ACTUAL.EUR.USD.FX')
    for info in r.sets(book):
        info['inputs']['curves'].append(copy.deepcopy(curve));new=copy.deepcopy(h);newpos=copy.deepcopy(p);info['holdings'].append(new);info['point']['positions'].append(newpos);direct=copy.deepcopy(fx);direct['snapshot_id']=info['point']['snapshot_id'];info['point']['fx']=[direct];r.c.set_contexts(dict(point=info['point'],holdings=[new]),info['point']['valuation_date'],info['point']['known_at'])
    opening.append(claims);opening._events.append(len(h['lifecycle']['events']));owners=copy.deepcopy(case['request']['operation']['input']['cohorts']);owners.append(dict(trade_id=h['id'],quantity='-700',acquisition=dict(kind='opening'),disposal_fill=None,entitlements=[dict(key=r.c.key(x['kind'],x['effective'],x['ordinal']),entitled=True,source=r.c.pin('COHORT.EUR.CASH.ENTITLEMENT.'+str(i))) for i,x in enumerate(claims)]))
    with localcontext() as context:
        context.prec=60;old_native=r.native;old_claims=r.inferred;r.native=cash.native;r.inferred=s.inferred
        try:expected=r.financial(book,owners,[],opening)
        finally:r.native=old_native;r.inferred=old_claims
    req=r.c.request(book['before'],[],[]);req['operation']['input']=book;req=wire.normalize(binding.encode(req));req['operation']=dict(kind='cohort_book_pnl',input=dict(book=req['operation']['input'],cohorts=wire.normalize(owners),physical_deliveries=[]));money.units(req);data['cases'].append(dict(id='cohort.full.native_USD_EUR_cash_ctd',request=req,expected_error=None,expected=expected));data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Original full dual-currency financial controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
