"""Complete separately supplied administrator observations at later cutoffs.

Preserve the original missing-publication controls. New complete controls supply
the exact previously absent eligible observation, independently recomputing full
dated futures/book prices and reconciliation. No candidate output or tolerance
is read or substituted. Fixing receipts enter the fixing phase; Time replays the
same original received observations, including the original opening-day reset.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_qualification as q
import causal_pnl_ready as binding
import causal_pnl_wire as wire

SOURCE=c.FOLDER/'causal-pnl-credit-dates.json'
TARGET=SOURCE.with_name('causal-pnl-fixing-cutoffs.json')
NAMES=['causal.fixing.overnight_eligible_complete_window','causal.event.actual_margin_final_and_receipts','causal.full.time_fixing_cash_ctd.actual_cash']
clone=copy.deepcopy

def generate():
    data=json.loads(SOURCE.read_bytes());originals={item['id']:clone(item) for item in data['cases']}
    for item in data['cases']:
        if item['id'] in NAMES:item['expected_error']='missing_input';item.pop('expected',None)
    with localcontext() as context:
        context.prec=60
        for name in NAMES:
            req=wire.expand(originals[name]['request']);inp=req['operation']['input']
            sets=[inp['before']]+[s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available']
            opening_history=clone(inp['before']['inputs']['fixings']);common_added=[]
            for info in sets:
                if info is not sets[0]:
                    info['inputs']['fixings'][len(opening_history):len(opening_history)]=clone(common_added)
                indices={h['product']['product']['input']['contract']['settlement']['accrual']['index'] for h in info['holdings'] if h['product'].get('product',{}).get('kind')=='rate_future'}
                existing={(r['index_id'],r['observation_date']) for r in info['inputs']['fixings']};added=[]
                for pub in info['inputs']['publications']:
                    if pub['index_id'] in indices and pub['expected_publication']<=info['point']['known_at'] and (pub['index_id'],pub['observation_date']) not in existing:
                        obs=dict(index_id=pub['index_id'],observation_date=pub['observation_date'],rate='0.0385',published_at=pub['expected_publication'],known_at=pub['expected_publication'],revision=1,status='Final',source_id='CAUSAL.ACTUAL.ADMINISTRATOR')
                        info['inputs']['fixings'].append(obs);added.append(obs);existing.add((pub['index_id'],pub['observation_date']))
                if info is sets[0]:common_added=clone(added)
                for h in info['holdings']:
                    p=h['product'].get('product',h['product'])
                    if p['kind']!='rate_future':continue
                    actual=[r for r in added if r['index_id']==p['input']['contract']['settlement']['accrual']['index']]
                    if actual:p['input']['fixing_stamp']['version']+=40
                    if any(s['event']['action']['kind']=='fixing' for s in h['lifecycle']['events']):
                        for obs in actual:
                            if obs['observation_date']>info['point']['valuation_date']:continue
                            step=c.append_event(h,obs['observation_date'],'fixing',obs,known=info['point']['known_at']);h['lifecycle']['events'].pop();step['event']['event_id']='causal.complete.administrator.'+obs['index_id']+'.'+obs['observation_date']
                            pos=next((i for i,s in enumerate(h['lifecycle']['events']) if s['event']['action']['kind']=='fixing' and s['event']['effective_date']>=obs['observation_date']),len(h['lifecycle']['events']))
                            h['lifecycle']['events'].insert(pos,step)
                        for i,step in enumerate(h['lifecycle']['events']):step['event']['expected_revision']=i
            # These are exactly the previously independently declared opening
            # claims, with no native output used to infer cash.
            _,claims=c.main_before();opening=c.ClaimSet(claims if name.startswith('causal.full.') else [[]])
            opening._events=[len(h['lifecycle']['events']) for h in inp['before']['holdings']]
            old=a.native;a.native=q.native
            try:expected=a.financial(req,opening)
            finally:a.native=old
            data['cases'].append(dict(id=name+'.complete_publications',request=wire.normalize(binding.encode(req)),expected_error=None,expected=expected))
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Original later-cutoff missing-publication controls retained. New exact administrator-observation controls independently recompute all complete prices, claims, actual cash, CTD and P&L at unchanged original bounds. No candidate output or earlier artifact changed.')
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args()
    raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent publication controls differ')
    data=json.loads(raw);print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
