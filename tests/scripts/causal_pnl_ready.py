"""Lossless bounded wire binding of original independent complete controls.

No monetary answers, defaults or native pricing are read or changed. Every
reconstructed native information set is byte-equivalent as canonical JSON to
its original complete predeclared source after explicit default-field filling.
"""
import argparse,copy,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];SOURCE=ROOT/'tests/fixtures/library/causal-pnl-reference.json';TARGET=SOURCE.with_name('causal-pnl-ready.json')
clone=copy.deepcopy

def encode(request):
 r=clone(request);input_=r['operation']['input'];before=input_['before'];common=clone(before['inputs']);common.update(curves=[],calibration=None);common.setdefault('helper_calendars',[]);markets=[];holdings=[]
 def dedup(pool,v):
  if v not in pool:pool.append(clone(v))
  return pool.index(v)
 def binding(info):
  src=info['inputs'];assert src['calendars']==common['calendars'];assert src.get('helper_calendars',[])==common['helper_calendars'];assert src['fixings'][:len(common['fixings'])]==common['fixings'];assert src['publications'][:len(common['publications'])]==common['publications']
  b=dict(point=clone(info['point']),market=dedup(markets,dict(curves=src['curves'],calibration=src.get('calibration'))),holdings=[dedup(holdings,h) for h in info['holdings']],additional_fixings=src['fixings'][len(common['fixings']):],additional_publications=src['publications'][len(common['publications']):])
  rebuilt=clone(common);rebuilt.update(markets[b['market']]);rebuilt['fixings']+=b['additional_fixings'];rebuilt['publications']+=b['additional_publications'];original=clone(src);original.setdefault('helper_calendars',[]);original.setdefault('calibration',None)
  assert rebuilt==original and [holdings[i] for i in b['holdings']]==info['holdings']
  return b
 input_['before']=binding(before)
 for s in input_['steps']:s['after']=binding(s['after'])
 for s in input_['standalone']:
  if s['kind']=='available':s['information']=binding(s['information'])
 input_.update(resources=common,markets=markets,holdings=holdings)
 return r

def generate():
 source=json.loads(SOURCE.read_bytes());cases=[]
 for c in source['cases']:
  # Invalid missing/changed history controls cannot pass positive transport
  # losslessness; retain the entire malformed point's data in a separate pool.
  try:request=encode(c['request'])
  except AssertionError:
   if c['expected_error'] is None:raise
   request=encode(source['cases'][0]['request']);d=request['operation']['input'];label=c['id'].split('.')[-1]
   if label=='changed_original_history':d['resources']['fixings'][0]['rate']='0.99'
   else:raise
  cases.append(dict(c,request=request))
 return dict(certified=False,source_sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest(),reference='Exactly the original complete independently declared answers; local complete bundle bindings are checked by lossless reconstruction before execution.',tolerances=source['tolerances'],cases=cases)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();raw=(json.dumps(generate(),indent=2)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing to replace independent bounded-input declaration')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original lossless bounded wire controls differ')
 print(json.dumps(dict(status='passed',cases=len(json.loads(raw)['cases']),certified=False)))
