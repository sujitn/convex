"""Separate exact native trade identity binding; original legal terms unchanged.
The initial portfolio labels are not the actual original IRS/CDS trade IDs.
Bind holding/state/event/fill IDs to those existing native IDs, and retain every
original monetary answer, legal term, source revision and comparison bound.
"""
import argparse,hashlib,json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library';SOURCE=FOLDER/'causal-pnl-publications.json';TARGET=SOURCE.with_name('causal-pnl-identities.json')
def generate():
 data=json.loads(SOURCE.read_bytes())
 for case in data['cases']:
  mapping={}
  for h in case['request']['operation']['input']['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind'] in ['swap','cds']:mapping[h['id']]=p['terms']['trade_id']
  def bind(x):
   if isinstance(x,dict):return {k:bind(v)for k,v in x.items()}
   if isinstance(x,list):return [bind(v)for v in x]
   return mapping.get(x,x)if isinstance(x,str)else x
  case['request']=bind(case['request'])
  if case['expected_error'] is None:case['expected']=bind(case['expected'])
 data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Exact same original native IRS/CDS trade identities bound to expected holdings/states/events/fills. Full legal economics, source revisions, all monetary answers and comparison bounds unchanged; original aliases and failures retained.')
 return data
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original exact native trade identities differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
