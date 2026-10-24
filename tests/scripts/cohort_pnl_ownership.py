"""Original pre-acquisition cash cannot become interval received cash.

Retain every previous complete financial book and failure. Independently declare
full original recovery-claim acquisitions after partial or full actual payment,
with a time/event/position prefix. The complete original received history remains
visible and these combined old-cash/residual transfers fail explicitly; no paid
history is invented as a new cohort receipt. A separate residual claim owner is
required for this unsupported transfer. No native output is read.
"""
import argparse, copy, hashlib, json
import cohort_pnl_reference as r
import cohort_pnl_states as s
import cohort_pnl_wire as money
import causal_pnl_wire as wire
import causal_pnl_ready as binding
SOURCE=r.TARGET.with_name('cohort-pnl-cash-dates.json')
TARGET=SOURCE.with_name('cohort-pnl-ownership.json')
def generate():
    data=json.loads(SOURCE.read_bytes())
    case=next(x for x in data['cases'] if x['id']=='cohort.credit.acquired_actual_recovery_claim')
    original=s.expand(case['request']);owner=case['request']['operation']['input']['cohorts']
    for fraction,label in [('0.5','partial'),('1','full')]:
        before=copy.deepcopy(original['before'])
        timed=r.c.repoint(before,'cohort.claim.time.before_receipt.'+label)
        r.c.set_contexts(timed,'2026-10-15')
        paid=r.c.repoint(timed,'cohort.claim.paid.before_acquisition.'+label)
        cash=next(x for x in r.rows(case['expected']['trades'][0]['marks'][0]) if x['kind']=='recovery')
        r.c.payment(paid['holdings'][0],cash,cash['amount']*r.D(fraction),on='2026-10-15',known=r.c.instant('2026-10-15'))
        acquired=r.c.repoint(paid,'cohort.claim.acquired.after_receipt.'+label)
        r.execution(acquired,0,owner[0]['acquisition']['fill_id'],owner[0]['quantity'],'200000')
        req=r.c.request(before,[('time',timed),('event',paid),('position',acquired)],[None,None,None])
        encoded=wire.normalize(binding.encode(req));encoded['operation']=dict(kind='cohort_book_pnl',input=dict(book=encoded['operation']['input'],cohorts=copy.deepcopy(owner),physical_deliveries=[]));money.units(encoded)
        data['cases'].append(dict(id='cohort.invalid.acquired_historically_'+label+'_paid_claim',request=encoded,expected_error='invalid_input'))
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Whole original received-history transfer guards differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
