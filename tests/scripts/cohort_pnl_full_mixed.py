"""Full original nine-product five-cause cohort book, declared before execution.

Reuse the exact original complete market/model/FX/received-cash resources and
all nonlinear product/basket owners. Supply an explicit full original IRS-lot
disposal with signed actual consideration and fees, instead of a partial fill
within one lot. Keep separate current/pending/projected/executed meanings and
all original monetary/cash/rate/points/Gaussian bounds. Independently compute
every native field, cash key/date/receipt/PV/removal, all-member CTD and ordered/
standalone/interaction/residual. No native executable or financial answer is read.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import cohort_pnl_calendars as calendars
import cohort_pnl_contracts as fixed
import cohort_pnl_cash_dates as accrued
import cohort_pnl_bindings as kind
import causal_pnl_wire as wire
import causal_pnl_ready as binding
SOURCE=r.SOURCE
TARGET=SOURCE.with_name('cohort-pnl-full-mixed.json')
def generate():
    base=next(x for x in json.loads(SOURCE.read_bytes())['cases'] if x['id']=='causal.full.mixed')
    book=wire.expand(base['request'])['operation']['input']
    opening=r.opening_from(base,book)
    quantity=D(book['before']['point']['positions'][1]['current']['quantity'])
    for info in r.sets(book):
        pos=info['point']['positions'][1];pos['pending_quantity']='0'
        for fill in info['point']['fills']:
            assert fill['trade_id']==pos['identity']['id']
            fill.update(id='COHORT.ACTUAL.FULL.IRS.DISPOSAL',quantity=str(-quantity),consideration=r.c.money('3500'),fees=r.c.money('-2'),source='cohort.synthetic.actual-full-disposal')
            pos['current']['quantity']='0'
    with localcontext() as context:
        context.prec=60;prior=(r.c.de.cash,r.native,r.inferred)
        r.c.de.cash=accrued.cash;r.native=fixed.native;r.inferred=calendars.inferred
        try:
            owners=r.owners_for(book,opening);owners[1]['disposal_fill']='COHORT.ACTUAL.FULL.IRS.DISPOSAL'
            expected=r.financial(book,owners,[],opening)
            assert not any(x['key']['kind']=='termination' for t in expected['trades'] for m in t['marks'] for x in m['claims'])
            kind.explain(expected,{},book)
        finally:r.c.de.cash,r.native,r.inferred=prior
    req=r.c.request(book['before'],[],[]);req['operation']['input']=book
    encoded=wire.normalize(binding.encode(req));encoded['operation']=dict(kind='cohort_book_pnl',input=dict(book=encoded['operation']['input'],cohorts=wire.normalize(owners),physical_deliveries=[]))
    expected['delivery_comparisons']=[]
    case=dict(id='cohort.full.all_five_causes_full_irs_disposal',request=encoded,expected_error=None,expected=expected)
    return dict(certified=False,reference=__doc__,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),tolerances=json.loads(r.TARGET.read_bytes())['tolerances'],cases=[case])
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Full five-cause original cohort financial controls differ')
    print(json.dumps(dict(status='passed',cases=1,positive=1,complete_marks=99,certified=False)))
