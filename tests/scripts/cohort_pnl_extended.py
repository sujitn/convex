"""Separate complete original acquisitions before earned cash and full termination.

New financial books are computed before their native execution. Earlier full
financial inputs, failed wire trials and numeric bounds remain immutable.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import cohort_pnl_reference as r
import cohort_pnl_states as s
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=r.TARGET.with_name('cohort-pnl-wire.json');TARGET=SOURCE.with_name('cohort-pnl-extended.json')
clone=copy.deepcopy
def generate():
    data=json.loads(SOURCE.read_bytes())
    def add(name,book,owners,opening):
        expected=r.financial(book,owners,[],opening);req=r.c.request(book['before'],[],[]);req['operation']['input']=book;req=wire.normalize(binding.encode(req));req['operation']=dict(kind='cohort_book_pnl',input=dict(book=req['operation']['input'],cohorts=wire.normalize(owners),physical_deliveries=[]))
        from cohort_pnl_wire import units
        units(req);data['cases'].append(dict(id=name,request=req,expected_error=None,expected=expected))
    with localcontext() as context:
        context.prec=60
        old=json.loads(r.SOURCE.read_bytes());case=next(x for x in old['cases'] if x['id']=='causal.time.stepped_delayed_partial.plain_contract');book=wire.expand(case['request'])['operation']['input'];opening=r.opening_from(case,book);h=clone(book['before']['holdings'][0]);p=clone(book['before']['point']['positions'][0]);h['id']='cohort.cash.before_boundary';p['identity'].update(id=h['id'],economics=r.c.pin(h['id']+'.economics'),calculation=r.c.pin(h['id']+'.calculation'));p['current']['quantity']='600';p['pending_quantity']='0';h,claims=r.c.lifecycle(h,p,book['before']['inputs'],book['before']['point']['valuation_date']);p['current']['quantity']='0';book['before']['holdings'].append(h);book['before']['point']['positions'].append(p);opening.append(claims);opening._events.append(len(h['lifecycle']['events']));acquired=r.c.repoint(book['before'],'cohort.cash.acquire_before_boundary');r.execution(acquired,1,'ACTUAL.CASH.BEFORE.CASH.ACQUISITION','600','-600.25');boundary=r.c.repoint(acquired,'cohort.cash.new_earned_boundary');r.c.set_contexts(boundary,'2024-07-02');r.c.append_event(boundary['holdings'][0],'2024-07-02','contract_step',known=r.c.instant('2024-07-02'));r.c.append_event(boundary['holdings'][0],'2024-07-02','contractual_cash',known=r.c.instant('2024-07-02'));r.c.append_event(boundary['holdings'][1],'2024-07-02','contract_step',known=r.c.instant('2024-07-02'));r.c.append_event(boundary['holdings'][1],'2024-07-02','contractual_cash',known=r.c.instant('2024-07-02'));cash=r.c.repoint(boundary,'cohort.cash.new_cash_date');r.c.set_contexts(cash,'2024-07-07');book['steps']=[dict(cause='position',after=acquired),dict(cause='time',after=boundary),dict(cause='time',after=cash)];book['standalone']=[dict(kind='unavailable',reason='Original acquisition and earned boundaries require their actual causal prefix.') for _ in book['steps']];fake=clone(book);fake['before']['point']['positions'][1]['current']['quantity']='600';owners=r.owners_for(fake,opening);owners[1].update(quantity='600',acquisition=dict(kind='fill',fill_id='ACTUAL.CASH.BEFORE.CASH.ACQUISITION'));owners[1]['entitlements']=[dict(x,entitled=x['key']['effective_date']>'2024-07-01') for x in owners[1]['entitlements']];add('cohort.cash.acquired_before_earned_cash',book,owners,opening)
        case=next(x for x in data['cases'] if x['id']=='cohort.irs.partial_termination_disposal_cash');book=s.expand(case['request']);opening=r.c.ClaimSet([r.rows(t['marks'][0]) for t in case['expected']['trades']]);opening._events=[len(h['lifecycle']['events']) for h in book['before']['holdings']]
        for info in r.sets(book):
            info['point']['positions'][0]['pending_quantity']='0'
            for step in info['holdings'][0]['lifecycle']['events']:
                if step['event']['action']['kind']=='terminate':step['event']['action']['terms'].update(fraction='1',agreement=r.c.pin('ACTUAL.FULL.TERMINATION'))
        owners=clone(case['request']['operation']['input']['cohorts']);add('cohort.irs.full_termination_disposal_cash',book,owners,opening)
    # Typed admission stays separate from independent financial answers.
    def reject(name,fn,source='cohort.full.acquired_after_earned_coupon_ctd',code='invalid_input'):
        x=clone(next(x for x in data['cases'] if x['id']==source));x.update(id=name,expected_error=code);x.pop('expected');fn(x['request']['operation']['input']);data['cases'].append(x)
    reject('cohort.invalid.entitlement_budget',lambda x:x['cohorts'][-1].update(entitlements=x['cohorts'][-1]['entitlements']*4097))
    reject('cohort.invalid.unknown_owner_field',lambda x:x['cohorts'][0].update(price=100))
    reject('cohort.invalid.unknown_entitlement_field',lambda x:x['cohorts'][0]['entitlements'][0].update(cash=100))
    reject('cohort.invalid.no_original_claim',lambda x:x['cohorts'][0].update(entitlements=[]),code='missing_input')
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('New full independent ownership controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
