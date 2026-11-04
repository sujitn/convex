"""Separate exact original futures curve/source numeric revision binding.
Original changed market values already carry new curve revisions. Supply those
same explicit revisions in the original futures projection stamps; a nonnumeric
curve alias cannot substitute for the original native revision. Monetary answers,
legal economics and all bounds are unchanged; raw failed bindings remain.
"""
import argparse,hashlib,json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library';SOURCE=FOLDER/'causal-pnl-identities.json';TARGET=SOURCE.with_name('causal-pnl-revisions.json')
def generate():
 d=json.loads(SOURCE.read_bytes())
 def revision(x):
  if isinstance(x,dict):return {k:revision(v)for k,v in x.items()}
  if isinstance(x,list):return [revision(v)for v in x]
  return '2'if x=='causal.market.2'else x
 d=revision(d)
 for case in d['cases']:
  for h in case['request']['operation']['input']['holdings']:
   p=h['product'].get('product',h['product'])
   if p['kind']=='rate_future':p['input']['projection']['stamp']['version']=int(p['input']['projection']['curve']['version'])
 d.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Exact actual numeric curve/source revision bindings on unchanged original complete market/economic inputs and monetary answers/bounds. Raw original nonnumeric or mismatched revisions retained; no native outputs read.')
 return d
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original exact numeric source revisions differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
