"""Separate original complete optional-index publication/fixing scope admission.
Reuse the already declared original mixed-book disjoint internal index mapping.
Original contradictory combined decisions remain immutable. No monetary values,
parameters, dates, history revisions or bounds change; no native outputs read.
"""
import argparse,copy,hashlib,json
from pathlib import Path
import mixed_scenario_reference as m
from mixed_scenario_admission import rename
FOLDER=m.HERE;SOURCE=FOLDER/'causal-pnl-terms.json';TARGET=SOURCE.with_name('causal-pnl-publications.json')
def generate():
 data=json.loads(SOURCE.read_bytes());optional=m.all_dates(copy.deepcopy(m.cases('optional-graph-hedge-resolved.json')['optional_graph.frn.call']),1);resources=optional['inputs'];projection=optional['operation']['product']['input']['projection']
 for case in data['cases']:
  inp=case['request']['operation']['input'];common=inp['resources']
  present=any(p in common['publications'] for p in resources['publications'])
  if not present:continue
  for name in ['publications','fixings']:
   if not any(item in common[name] for item in resources[name]):continue
   common[name]=[x for x in common[name] if x not in resources[name]]+[rename(x) for x in resources[name]]
  for market in inp['markets']:
   for curve in market['curves']:
    if curve['key']==projection:curve['role']=rename(curve['role'])
  for h in inp['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind']!='optional_bond':continue
   p['input']['bond']['terms']['coupon']=rename(p['input']['bond']['terms']['coupon']);h['lifecycle']['instrument']['bond']['terms']['coupon']=rename(h['lifecycle']['instrument']['bond']['terms']['coupon']);h['lifecycle']['history']=[rename(o)for o in h['lifecycle']['history']if o in resources['fixings']]
  if case['expected_error'] is None:
   for t in case['expected']['trades']:
    for mark in t['marks']:
     if mark['native']['kind']=='optional':mark['native']['full']['known']=rename(mark['native']['full']['known'])
 data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Separate already declared disjoint optional internal reset-index/publication/fixing/curve-role namespace. All full original financial numbers, parameters, dates, revision values and comparison bounds remain exact; contradictory original source preparations retained; no native output read.')
 return data
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Complete disjoint publication inputs differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
