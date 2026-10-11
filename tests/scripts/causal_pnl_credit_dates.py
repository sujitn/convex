"""Separate eligible original CDS default dates and independent reconciliation.

The original pre-protection defaults remain typed invalid inputs. Two new full
controls start inside the SAME legal protection interval, with actual default,
auction and receipt dates. Independently recompute the complete dated prices,
default accrual, initial cash, claims and P&L; do not read candidate output or
change the original terms, comparison bounds or previous declarations.
"""
import argparse, copy, hashlib, json
from decimal import Decimal as D, localcontext
from pathlib import Path
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_ready as binding
import causal_pnl_wire as wire

FOLDER=c.FOLDER
SOURCE=FOLDER/'causal-pnl-cash-contracts.json'
TARGET=SOURCE.with_name('causal-pnl-credit-dates.json')
clone=copy.deepcopy

def generate():
    data=json.loads(SOURCE.read_bytes())
    originals={item['id']:clone(item) for item in data['cases']}
    for item in data['cases']:
        if item['id'] in ['causal.event.credit_recovery','causal.event.credit_complete_actual_cash','causal.invalid.unknown_recovery']:
            item['expected_error']='invalid_input';item.pop('expected',None)
    with localcontext() as context:
        context.prec=60
        for name in ['causal.event.credit_recovery','causal.event.credit_complete_actual_cash']:
            req=wire.expand(originals[name]['request']);inp=req['operation']['input']
            sets=[inp['before']]+[s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available']
            protected=inp['before']['holdings'][0]['product']['product']['terms']['protection_start_date']
            for info in sets:
                if info['point']['valuation_date']<protected:
                    c.set_contexts(info,protected)
                for h in info['holdings']:
                    for step in h['lifecycle']['events']:
                        event=step['event'];act=event['action']
                        if act['kind'] in ['credit_default','recovery']:
                            event.update(effective_date=protected,known_at=c.instant(protected))
                        if act['kind']=='payment' and act['terms']['obligation']['kind'] in ['recovery','default_accrual']:
                            act['terms']['obligation']['effective_date']=protected
            # Payment facts in the new complete-cash case must refer to its new
            # independently calculated accrual, not the rejected date's amount.
            _,original_claims=c.main_before();opening=c.ClaimSet([original_claims[6]])
            opening._events=[len(inp['before']['holdings'][0]['lifecycle']['events'])]
            for info in sets:
                for step in info['holdings'][0]['lifecycle']['events']:
                    act=step['event']['action']
                    if act['kind']=='payment' and act['terms']['obligation']['kind']=='default_accrual':
                        probe=clone(info);probe['holdings'][0]['lifecycle']['events']=[s for s in probe['holdings'][0]['lifecycle']['events'] if s['event']['action']['kind']!='payment']
                        rows,_,_,_=a.claims(probe,0,opening)
                        act['terms']['amount']['value']=c.decstr(next(r['amount'] for r in rows if r['kind']=='default_accrual'))
            expected=a.financial(req,opening)
            item=dict(id=name+'.eligible_protection_date',request=wire.normalize(binding.encode(req)),expected_error=None,expected=expected)
            data['cases'].append(item)
            if name.endswith('credit_recovery'):
                missing=clone(item);missing.update(id='causal.invalid.eligible_default_unknown_recovery',expected_error='missing_input');missing.pop('expected')
                d=missing['request']['operation']['input']
                for h in d['holdings']:
                    h['lifecycle']['events']=[s for s in h['lifecycle']['events'] if s['event']['action']['kind']!='recovery']
                data['cases'].append(missing)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Original ineligible pre-protection defaults remain typed invalid inputs. Separate same-full-contract eligible-date controls independently recompute all dated monetary fields and receipts at unchanged original bounds. No candidate output, legal terms or earlier artifact changed.')
    return data

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args()
    raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent eligible-date controls differ')
    data=json.loads(raw)
    print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
