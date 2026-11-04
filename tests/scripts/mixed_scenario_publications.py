"""Separate publication-scope admission for full delivery-only controls.

Unused optional resources are retained with their disjoint original identities.
Original independently declared financial answers and tolerances remain exact.
"""
import argparse,copy,hashlib,json
from pathlib import Path
import mixed_scenario_reference as r
from mixed_scenario_admission import rename
ROOT=r.ROOT;HERE=r.HERE;SOURCE=HERE/'mixed-scenario-admitted.json';TARGET=HERE/'mixed-scenario-publications.json'
def generate():
 v=json.loads(SOURCE.read_bytes());original=r.all_dates(copy.deepcopy(r.cases('optional-graph-hedge-resolved.json')['optional_graph.frn.call']),1)
 projection=original['operation']['product']['input']['projection'];resources=original['inputs']
 for case in v['cases']:
  req=case['request']
  for name in ['publications','fixings']:
   if not any(item in resources[name] for item in req['inputs'][name]):continue
   req['inputs'][name]=[item for item in req['inputs'][name] if item not in resources[name]]
   for item in rename(resources[name]):
    if item not in req['inputs'][name]:req['inputs'][name].append(item)
  for curve in req['inputs']['curves']:
   if curve['key']==projection:curve['role']=rename(curve['role'])
  if case['expected_error'] is None:
   actual=json.loads(json.dumps(r.financial(req),default=r.de.serial)) if case['id']!='mixed.delivery.multiple_quote_rebuild' else None
   if actual is not None and actual!=case['expected']:raise ValueError('Original full financial values changed: '+case['id'])
 v['source_reads']['tests/fixtures/library/mixed-scenario-admitted.json']=hashlib.sha256(SOURCE.read_bytes()).hexdigest()
 v['qualification']+=' Disjoint optional publication/fixing/curve-role resources are retained even where the original full control has no optional position; original full financial answers unchanged.'
 return v
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,separators=(',',':'))+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Publication declaration changed')
 print(json.dumps(dict(status='passed',cases=len(v['cases']),certified=False)))
