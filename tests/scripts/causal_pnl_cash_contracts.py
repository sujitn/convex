"""Separate full plain amortizing cash contract; conditional original preserved.
The lifecycle reference bond has an American call. Pricing it through the plain
cash owner is correctly unsupported. Retain that complete conditional request
as a failure. Separately declare a full plain bond with exactly the same stepped
coupon/principal/rounding/delay terms and independent cash answers; a new
instrument revision explicitly has no optional provision. No native pricing,
original contract, financial bound or earlier artifact is changed.
"""
import argparse,copy,hashlib,json
from pathlib import Path
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library';SOURCE=FOLDER/'causal-pnl-revisions.json';TARGET=SOURCE.with_name('causal-pnl-cash-contracts.json')
def generate():
 d=json.loads(SOURCE.read_bytes());original=next(c for c in d['cases']if c['id']=='causal.time.stepped_delayed_partial');plain=copy.deepcopy(original);original['expected_error']='unsupported';original.pop('expected',None);plain['id']+=' .plain_contract';plain['id']=plain['id'].replace(' ','');inp=plain['request']['operation']['input']
 for h in inp['holdings']:
  h['product']['product']['bond']['terms']['exercise']=[];h['lifecycle']['instrument']['bond']['terms']['exercise']=[];h['lifecycle']['version']=2
 for b in [inp['before']]+[s['after']for s in inp['steps']]+[s['information']for s in inp['standalone']if s['kind']=='available']:
  b['point']['positions'][0]['identity']['instrument']={'id':'CAUSAL.PLAIN.STEPPED.CASH','version':2}
 d['cases'].append(plain);d.update(source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Original American conditional bond remains complete and typed unsupported by the plain pricing owner. New explicitly supplied full plain stepped/principal/delayed/rounded bond revision has the separately unchanged independent cash answers/bounds. No original terms or numerical owner changed.')
 return d
if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing overwrite')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original full plain/conditional contracts differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
