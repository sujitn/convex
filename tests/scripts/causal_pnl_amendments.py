"""Separately declared future-effective complete profile amendment.

The original amendment creates an immediately due, unreplayed profile boundary
and remains typed invalid. Supply a new explicit future-effective profile cut,
preserving all actual past economics and complete terms, and independently
recompute full bond cash/PV and ordered/standalone reconciliation. No candidate
outputs, earlier controls, original financial bounds or lifecycle owners change.
"""
import argparse,copy,hashlib,json
from decimal import localcontext
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_ready as binding
import causal_pnl_wire as wire
SOURCE=c.FOLDER/'causal-pnl-notice-dates.json'
TARGET=SOURCE.with_name('causal-pnl-amendments.json')
def generate():
    data=json.loads(SOURCE.read_bytes());original=next(x for x in data['cases'] if x['id']=='causal.amendment.full_prospective_profile');new=copy.deepcopy(original);original['expected_error']='invalid_input';original.pop('expected')
    req=wire.expand(new['request']);inp=req['operation']['input'];sets=[inp['before']]+[s['after'] for s in inp['steps']]+[s['information'] for s in inp['standalone'] if s['kind']=='available']
    for info in sets:
        for h in info['holdings']:
            h['product']['product']['_independent_lifecycle']=True
            for terms in [h['product']['product']['bond']['terms'],h['lifecycle']['instrument']['bond']['terms']]:
                rows=terms['profiles']['coupon']
                if len(rows)==2:rows[0]['end']='2025-02-01';rows[1]['start']='2025-02-01'
    with localcontext() as context:
        context.prec=60;opening=c.ClaimSet([[]]);opening._events=[0];new['expected']=a.financial(req,opening)
    for info in sets:
        for h in info['holdings']:h['product']['product'].pop('_independent_lifecycle')
    new['id']+='.future_effective';new['request']=wire.normalize(binding.encode(req));data['cases'].append(new)
    data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Original immediate unreplayed amendment remains typed invalid. Separate complete future-effective prospective profile change has full independent dated cash/PV/attribution at unchanged original bounds, no candidate outputs or earlier artifacts changed.')
    return data
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();raw=(json.dumps(generate(),indent=2,default=c.de.serial)+'\n').encode()
    if args.write:
        if TARGET.exists():raise ValueError('Refusing overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw:raise ValueError('Independent full future-effective amendment differs')
    data=json.loads(raw);print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(x['expected_error'] is None for x in data['cases']),certified=False)))
