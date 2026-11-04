"""Separate complete optional-index publication/fixing scope admission.

Original contradictory combined publications remain immutable and rejected.
No native output is read. Financial values/tolerances remain identical; only
explicitly supplied optional reset-index/source identities are disambiguated.
"""
import argparse,copy,hashlib,json
from pathlib import Path
import mixed_scenario_reference as r
ROOT=r.ROOT;HERE=r.HERE;TARGET=HERE/'mixed-scenario-admitted.json'
OLD='USD.TERM6M';NEW='USD.SCENARIO.TERM6M'
def rename(v):
 if isinstance(v,dict):return {k:rename(x) for k,x in v.items()}
 if isinstance(v,list):return [rename(x) for x in v]
 return NEW if v==OLD else v

def generate():
 source=HERE/'mixed-scenario-reference.json';v=json.loads(source.read_bytes());optional=r.all_dates(copy.deepcopy(r.cases('optional-graph-hedge-resolved.json')['optional_graph.frn.call']),1)['inputs']
 pubs=optional['publications'];fixings=optional['fixings']
 for case in v['cases']:
  req=case['request'];data=req['operation']['action']['input'];products=[h['product']['product'] for h in data['holdings'] if h['product']['kind']=='product' and h['product']['product']['kind']=='optional_bond']
  if not products:continue
  projection=products[0]['input']['projection']
  for p in products:p['input']['bond']['terms']['coupon']=rename(p['input']['bond']['terms']['coupon'])
  for c in req['inputs']['curves']:
   if c['key']==projection:c['role']=rename(c['role'])
  req['inputs']['publications']=[p for p in req['inputs']['publications'] if p not in pubs]+rename(pubs)
  req['inputs']['fixings']=[p for p in req['inputs']['fixings'] if p not in fixings]+rename(fixings)
  if case['expected_error'] is None:
   expected=rename(case['expected'])
   actual=json.loads(json.dumps(r.financial(req),default=r.de.serial))
   if actual!=expected:raise ValueError('Original full financial values changed: '+case['id'])
   case['expected']=expected
 v['source_reads']['tests/fixtures/library/mixed-scenario-reference.json']=hashlib.sha256(source.read_bytes()).hexdigest()
 v['qualification']='Disjoint actual optional reset-index, curve-role, administrator publication and eligible fixing identities. Original full combined financial answers and numerical bounds remain exact; raw failed request/artifacts remain immutable.'
 return v
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,separators=(',',':'))+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing to overwrite admission')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original full admission differs')
 print(json.dumps(dict(status='passed',cases=len(v['cases']),bytes=len(raw),financial_values_unchanged=True,certified=False)))
