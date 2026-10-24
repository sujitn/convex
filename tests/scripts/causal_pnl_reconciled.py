"""Separate exact whole-history lineage and original initial-credit cash.

All failed requests, financial controls and bounds remain upstream. Bind new
synthetic administrator observations to the actual original future history
source, give identical whole history the same new manifest revision, preserve
economic contexts in the fixing-only phase, and independently calculate pending
initial CDS cash using its ORIGINAL trade step-in date. Recompute complete full
native economic controls without candidate output, residual fits or new bounds.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_qualification as q
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=c.FOLDER/'causal-pnl-amendments.json'
TARGET=SOURCE.with_name('causal-pnl-reconciled.json')
NAMES=['causal.event.credit_recovery.eligible_protection_date','causal.event.credit_complete_actual_cash.eligible_protection_date',
 'causal.fixing.overnight_eligible_complete_window.complete_publications',
 'causal.event.actual_margin_final_and_receipts.complete_publications',
 'causal.full.time_fixing_cash_ctd.actual_cash.complete_publications']

def native(info,h):
    result=q.native(info,h);p=h['product'].get('product',h['product'])
    if p['kind']=='cds' and p['context']['initial_payment_state']=='Pending':
        terms=p['terms'];context=copy.deepcopy(p['context']);context.update(step_in_date=terms['step_in_date'],cash_settlement_date=terms['cash_settlement_date'])
        holidays=next(x['holidays'] for x in info['inputs']['calendars'] if x['pin']==terms['premium_calendar'])
        discount=c.curve_zero(info['inputs'],terms['discount_curve']['id']);hazard=c.curve_zero(info['inputs'],terms['survival_curve']['id'])
        recovery=next(x['role']['recovery'] for x in info['inputs']['curves'] if x['key']==terms['survival_curve'])
        initial=D(c.cr.cds_price(terms,context,holidays,discount,hazard,D(str(recovery)))['initial'])
        full=result['full'];full['initial']=str(initial);full['pv']=str(D(full['premium'])+D(full['protection'])+initial);result['pv']=D(full['pv'])
    return result

def generate():
    data=json.loads(SOURCE.read_bytes())
    for item in data['cases']:
        if item['id']=='causal.boundary.unmodelled_overdue_claim':
            item['expected_error']='invalid_input';extra=copy.deepcopy(item);extra.update(id='causal.boundary.complete_unmodelled_overdue_claim',expected_error='missing_input')
            inp=extra['request']['operation']['input'];points=[inp['before']]+[s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available']
            for b in points:b['point']['positions'][0]['overdue']='reject'
            data['cases'].append(extra);break
    with localcontext() as context:
        context.prec=60
        for item in data['cases']:
            if item['id'] not in NAMES:continue
            req=wire.expand(item['request']);inp=req['operation']['input'];sets=[inp['before']]+[s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available'];manifests={}
            for info in sets:
                sources={h['product']['product']['input']['contract']['settlement']['accrual']['index']:h['product']['product']['input']['fixing_stamp']['source'] for h in info['holdings'] if h['product'].get('product',{}).get('kind')=='rate_future'}
                for obs in info['inputs']['fixings']:
                    if obs['index_id'] in sources:obs['source_id']=sources[obs['index_id']]
                history=json.dumps([x for x in info['inputs']['fixings'] if x['index_id'] in sources],sort_keys=True)
                if history not in manifests:manifests[history]=500+len(manifests)
                for h in info['holdings']:
                    p=h['product'].get('product',h['product'])
                    if p['kind']!='rate_future':continue
                    p['input']['fixing_stamp']['version']=manifests[history]
                    for obs in h['lifecycle']['history']:obs['source_id']=sources[obs['index_id']]
                    for step in h['lifecycle']['events']:
                        if step['event']['action']['kind']=='fixing':obs=step['event']['action']['terms'];obs['source_id']=sources[obs['index_id']]
            # Fixed-date receipt of observations does not amend the original
            # CDS economic quotation/settlement context.
            previous=inp['before']
            for step in inp['steps']:
                after=step['after']
                if step['cause']=='fixing':
                    for before_h,after_h in zip(previous['holdings'],after['holdings']):
                        if after_h['product'].get('product',{}).get('kind')=='cds':after_h['product']['product']['context']=copy.deepcopy(before_h['product']['product']['context'])
                previous=after
            rows=[]
            for trade in item['expected']['trades']:
                rows.append([dict(kind=r['key']['kind'],effective=r['key']['effective_date'],ordinal=r['key']['ordinal'],due=r['due'],amount=D(r['amount']),recorded=D(r['recorded'])) for r in trade['marks'][0]['claims']])
            opening=c.ClaimSet(rows);opening._events=[len(h['lifecycle']['events']) for h in inp['before']['holdings']];old=a.native;a.native=native
            try:item['expected']=a.financial(req,opening)
            finally:a.native=old
            item['request']=wire.normalize(binding.encode(req))
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Separate original whole-history source/revision and fixing-only economic-context binding; full pending initial-credit cash independently uses original contractual trade step-in date. Recomputed complete cash/prices/claims/CTD/P&L at original bounds, all previous failed preparations and financial controls preserved. No candidate outputs read. Additional complete overdue-policy failure distinguishes missing model from wrong cause.')
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent full original lineage/cash controls differ')
    data=json.loads(raw);print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
