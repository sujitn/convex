"""Separate exact Decimal wire admission and explicit actual cash precision.
Earlier requests/answers are immutable. Trim redundant numeric lexeme zeros and
add a leading zero without changing any financial value. An original full-book
receipt with60 significant decimal places is retained as a typed invalid-input
control. Its separate actual-cash case supplies explicit cent-rounded receipt
facts (not a rounding change to contractual cash), with independently recomputed
cash/claim/P&L controls at exactly the original bounds. No native output is read.
"""
import argparse,copy,hashlib,json,re
from decimal import Decimal as D,localcontext,ROUND_HALF_EVEN
from pathlib import Path
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_qualification as q
import causal_pnl_ready as binding
ROOT=c.ROOT;FOLDER=c.FOLDER;SOURCE=FOLDER/'causal-pnl-qualified.json';TARGET=SOURCE.with_name('causal-pnl-wire.json');clone=copy.deepcopy

def normalize(x,key=''):
 if isinstance(x,dict):return {k:normalize(v,k)for k,v in x.items()}
 if isinstance(x,list):return [normalize(v,key)for v in x]
 if isinstance(x,str) and key not in ['id','version','source','source_id','reason','evidence'] and re.fullmatch(r'-?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)',x):
  s=format(D(x),'f');s=s.rstrip('0').rstrip('.') if '.'in s else s
  if D(s)!=D(x):raise ValueError('Financial value changed under wire normalization')
  return s
 return x

def expand(request):
 r=clone(request);inp=r['operation']['input']
 def point(b):
  inputs=clone(inp['resources']);inputs.update(clone(inp['markets'][b['market']]));inputs['fixings']+=b['additional_fixings'];inputs['publications']+=b['additional_publications'];return dict(point=clone(b['point']),inputs=inputs,holdings=[clone(inp['holdings'][i])for i in b['holdings']])
 before=point(inp['before']);steps=[dict(cause=s['cause'],after=point(s['after']))for s in inp['steps']];standalone=[dict(kind='available',information=point(s['information']))if s['kind']=='available'else clone(s)for s in inp['standalone']];r['operation']['input']={k:clone(v)for k,v in inp.items()if k not in ['resources','markets','holdings','before','steps','standalone']};r['operation']['input'].update(before=before,steps=steps,standalone=standalone);return r

def generate():
 source=json.loads(SOURCE.read_bytes());cases=[]
 for item in source['cases']:
  item=clone(item);item['request']=normalize(item['request'])
  if item['id']=='causal.full.time_fixing_cash_ctd':item['expected_error']='invalid_input';item.pop('expected',None)
  cases.append(item)
 with localcontext()as context:
  context.prec=60;old=next(c for c in source['cases']if c['id']=='causal.full.time_fixing_cash_ctd');req=expand(old['request']);inp=req['operation']['input']
  for info in [s['after']for s in inp['steps']]+[s['information']for s in inp['standalone']if s['kind']=='available']:
   for h in info['holdings']:
    for step in h['lifecycle']['events']:
     if step['event']['action']['kind']=='payment':
      m=step['event']['action']['terms']['amount'];value=D(m['value']);n=normalize(m)['value'];fraction=n.partition('.')[2]
      if len(fraction)>28:m['value']=format(value.quantize(D('.01'),rounding=ROUND_HALF_EVEN),'f')
  _,cs=c.main_before();opening=c.ClaimSet(cs);opening._events=[len(h['lifecycle']['events'])for h in inp['before']['holdings']];old_native=a.native;a.native=q.native
  try:expected=a.financial(req,opening)
  finally:a.native=old_native
  cases.append(dict(id='causal.full.time_fixing_cash_ctd.actual_cash',request=normalize(binding.encode(req)),expected_error=None,expected=expected))
 return dict(certified=False,reference='Separate exact Decimal lexical admission, original overprecision input retained as typed failure, and independent complete actual-cash precision facts. Original contractual rounding, mathematical answers in unchanged cases and all comparison bounds preserved; no native output read.',source_reads={'tests/fixtures/library/causal-pnl-qualified.json':hashlib.sha256(SOURCE.read_bytes()).hexdigest()},tolerances=source['tolerances'],cases=cases)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=c.de.serial)+'\n').encode()
 if args.write:
  if TARGET.exists():raise ValueError('Refusing overwrite of separate cash controls')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Full Decimal cash inputs differ')
 print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(c['expected_error']is None for c in data['cases']),certified=False)))
