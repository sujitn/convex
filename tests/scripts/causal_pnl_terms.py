"""Separate complete native Money unit declaration over unchanged full controls."""
import argparse,hashlib,json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library';SOURCE=FOLDER/'causal-pnl-wire.json';TARGET=SOURCE.with_name('causal-pnl-terms.json')
def generate():
 data=json.loads(SOURCE.read_bytes())
 def units(x):
  if isinstance(x,dict):
   if set(x)=={'value','currency'}:x['unit']='currency'
   for v in x.values():units(v)
  if isinstance(x,list):
   for v in x:units(v)
 for case in data['cases']:units(case['request'])
 data.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Full native Money currency-unit declaration only, with exact original monetary answers, inputs, bounds and earlier failures retained. No native outputs read.')
 return data
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original complete Money units differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
