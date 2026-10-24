"""Original future claims remain unpaid until their actual native cash due date.

Retain the failed complete early-payment trial. Fixed-date discounted OIS/CDS
books exercise complete ownership transfers/returns without inventing early cash
or moving clocks past unavailable original product/history boundaries. Due cash
and recovery books retain their full actual receipt allocation controls.
All original native due-date, profile, model and financial bounds remain exact.
"""
import argparse,copy,hashlib,json
from decimal import localcontext
import claim_pnl_reference as original
import claim_pnl_complete as complete
import cohort_pnl_reference as r
import cohort_pnl_states as states

SOURCE=complete.TARGET;TARGET=SOURCE.with_name('claim-pnl-chronology.json')
clone=copy.deepcopy

def generate():
    data=json.loads(SOURCE.read_bytes())
    with localcontext() as ctx:
        ctx.prec=60
        for i,case in enumerate(data['cases']):
            if case['expected_error'] is not None:continue
            spec=case['request']['operation']['input'];book=states.expand(case['request'])
            transfer=spec['claim_transfers'][0]
            j=next(j for j,o in enumerate(spec['cohorts']) if o['trade_id']==transfer['origin'])
            first=transfer['applied_points'][0]
            cash=next(c for c in case['expected']['trades'][j]['marks'][first]['claims'] if c['key']==transfer['key'])
            if cash['due']<=book['steps'][first-1]['after']['point']['valuation_date']:continue
            for h in spec['book']['holdings']:
                h['lifecycle']['events']=[s for s in h['lifecycle']['events'] if not
                    (s['event']['action']['kind']=='payment' and s['event']['action']['terms']['receipt_id'].startswith('claim.'))]
            facts=spec['claim_transfers'];second=clone(facts[1]);facts.clear();facts.append(transfer)
            def extra(point,fraction,label):
                t=clone(transfer);t.update(id=case['id']+'.LEGAL.'+label,**{'from':transfer['to'],'to':transfer['origin']},
                    fraction=fraction,source=r.c.pin(case['id']+'.DOCUMENT.'+label),applied_points=list(range(point,len(spec['book']['steps'])+1)))
                spec['book']['steps'][point-1]['cause']='position';return t
            facts.extend([extra(first+1,'0.1','FIRST_RETURN'),second,extra(first+3,'0.15','FINAL_RETURN')])
            # No actual future receipt exists in this fixed-date information set.
            for t in facts:t['cutoff']['receipt_ids']=[]
            data['cases'][i]=complete.financial(case)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference=__doc__)
    for c in data['cases']:
        c['input_nodes']=original.lots.nodes(c['request']);c['input_bytes']=len(json.dumps(c['request'],separators=(',',':')).encode())
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();data=generate()
    raw=(json.dumps(data,indent=2,default=r.c.de.serial)+'\n').encode()
    if a.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Complete original unpaid future claim controls differ')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),financial_books=sum(c['expected_error'] is None for c in data['cases']),max_nodes=max(c['input_nodes'] for c in data['cases']),certified=False)))
