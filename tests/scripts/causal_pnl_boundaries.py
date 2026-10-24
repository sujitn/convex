"""Additional complete actual-fill/amendment controls and fail-closed boundaries.
All mathematical answers derive from the original independent dated primitives
before native execution. No monetary tolerances or earlier controls change.
"""
import argparse,copy,hashlib,json
from decimal import Decimal as D,localcontext
from pathlib import Path
import causal_pnl_reference as c
import causal_pnl_admitted as a
import causal_pnl_ready as wire
ROOT=c.ROOT;SOURCE=c.FOLDER/'causal-pnl-bindings.json';TARGET=SOURCE.with_name('causal-pnl-boundaries.json');clone=copy.deepcopy

def generate():
 original=json.loads(SOURCE.read_bytes());cases=[]
 def add(id_,req):
  inp=req['operation']['input'];opening=c.ClaimSet([[]]);opening._events=[0];expected=a.financial(req,opening)
  def strip(v):
   if isinstance(v,dict):v.pop('_independent_lifecycle',None);[strip(x) for x in v.values()]
   if isinstance(v,list):[strip(x) for x in v]
  strip(req);cases.append(dict(id=id_,request=wire.encode(req),expected_error=None,expected=expected))
 with localcontext() as context:
  context.prec=60
  before,_=c.main_before();before['holdings']=[before['holdings'][3]];before['point']['positions']=[before['point']['positions'][3]];before['point']['fx']=[];before=c.repoint(before,'actual_fill.before');after=c.repoint(before,'actual_fill.after');p=after['point']['positions'][0];p['current']['quantity']='-1';p['pending_quantity']='0';after['point']['fills']=[dict(id='CAUSAL.ACTUAL.FUTURE.FILL',trade_id=p['identity']['id'],instrument=p['identity']['instrument'],quantity='1',consideration=c.money('250000'),fees=c.money('-1.17'),date=after['point']['valuation_date'],known_at=after['point']['known_at'],source='CAUSAL.ACTUAL.EXECUTION.SOURCE')];add('causal.position.actual_futures_fill',c.request(before,[('position',after)],[c.repoint(after,'actual_fill.single')]))
  t=clone(next(x['request']['operation']['product']['input']['bond']['terms'] for x in c.load('tests/fixtures/library/optional-graph-hedge-resolved.json')['cases'] if x['id']=='optional_graph.zero.positive.call'));t['exercise']=[];resources=c.simple_resources(root='2025-01-01');on='2025-01-15';product=c.bond_product(t,on);before,_=c.fixture_info('amended_bond',product,t,resources,on,current='500',pending='-20');after=c.repoint(before,'amendment.prospective');t2=clone(t);row=t2['profiles']['coupon'][0];t2['profiles']['coupon']=[dict(row,end=on),dict(row,start=on,value='.03')];h=after['holdings'][0];h['product']['product']['bond']['terms']=clone(t2);h['lifecycle']['instrument']['bond']['terms']=clone(t2);h['lifecycle']['version']+=1;after['point']['positions'][0]['identity']['instrument']['version']+=1;add('causal.amendment.full_prospective_profile',c.request(before,[('amendment',after)],[c.repoint(after,'amendment.single')]))
 def negative(label,source,error,change):
  r=clone(source);change(r['operation']['input']);cases.append(dict(id='causal.boundary.'+label,request=r,expected_error=error))
 main=original['cases'][0]['request'];margin=next(x['request'] for x in original['cases'] if x['id']=='causal.event.actual_margin_final_and_receipts');event_hold=lambda d:d['holdings'][d['steps'][0]['after']['holdings'][0]]
 for label,error,change in [
  ('unknown_local_market','invalid_input',lambda d:d['before'].update(market=9999)),
  ('unknown_local_holding','invalid_input',lambda d:d['before']['holdings'].__setitem__(0,9999)),
  ('unused_market','invalid_input',lambda d:d['markets'].append(clone(d['markets'][0]))),
  ('unused_holding','invalid_input',lambda d:d['holdings'].append(clone(d['holdings'][0]))),
  ('common_hidden_curves','invalid_input',lambda d:d['resources'].update(curves=clone(d['markets'][0]['curves']))),
  ('foreign_manifest','invalid_input',lambda d:d['steps'][0]['after']['point'].update(manifest=clone(d['before']['point']['manifest']))),
  ('time_backwards','invalid_input',lambda d:d['steps'][0]['after']['point'].update(valuation_date='2026-10-06')),
  ('knowledge_backwards','invalid_input',lambda d:d['steps'][0]['after']['point'].update(known_at='2026-10-07T11:00:00Z')),
  ('same_curve_revision','invalid_input',lambda d:d['markets'][1]['curves'][0]['key'].update(version=d['markets'][0]['curves'][0]['key']['version'])),
  ('false_opening_quantity','invalid_input',lambda d:d['holdings'][0]['lifecycle']['position'].update(value='-900')),
  ('foreign_native_economics','invalid_input',lambda d:d['holdings'][0]['lifecycle']['instrument']['bond']['terms']['profiles']['coupon'][0].update(value='.9')),
  ('same_FX_revision','invalid_input',lambda d:d['steps'][2]['after']['point']['fx'][0]['observation'].update(version=d['before']['point']['fx'][0]['observation']['version'])),
  ('missing_actual_FX','invalid_input',lambda d:d['before']['point'].update(fx=[])),
  ('foreign_fill_cash','invalid_input',lambda d:d['steps'][3]['after']['point']['fills'][0]['consideration'].update(currency='EUR')),
  ('foreign_fill_security','invalid_input',lambda d:d['steps'][3]['after']['point']['fills'][0]['instrument'].update(version=99)),
  ('absent_counterfactual','invalid_input',lambda d:d['standalone'].pop()),
  ('counterfactual_zero_placeholder','invalid_input',lambda d:d['standalone'].__setitem__(0,dict(kind='unavailable',reason=''))),
  ('oversize_point_policy','invalid_input',lambda d:d.update(max_points=34)),
  ('oversize_product_policy','invalid_input',lambda d:d.update(max_product_evaluations=4225)),
  ('unmodelled_overdue_claim','missing_input',lambda d:d['before']['point']['positions'][0].update(overdue='reject')),
  ('ignored_scalar_PNL','invalid_input',lambda d:d.update(pnl=0)),
  ('ignored_supplied_mark','invalid_input',lambda d:d['before'].update(value=0)),
 ]:negative(label,main,error,change)
 negative('margin_clearing_absent',margin,'invalid_input',lambda d:event_hold(d)['lifecycle']['events'][-1].update(futures=None))
 actual_fill=cases[0]['request']
 negative('fractional_actual_future_fill',actual_fill,'invalid_input',lambda d:d['steps'][0]['after']['point']['fills'][0].update(quantity='.5'))
 negative('wrong_future_consideration_sign',actual_fill,'invalid_input',lambda d:d['steps'][0]['after']['point']['fills'][0]['consideration'].update(value='-250000'))
 negative('off_tick_actual_future_fill',actual_fill,'invalid_input',lambda d:d['steps'][0]['after']['point']['fills'][0]['consideration'].update(value='249999.991'))
 return dict(certified=False,reference='Separate full actual-future-fill and prospective profile amendment controls plus strict causal/source/unit/event boundaries. No candidate outputs.',source_reads={str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [SOURCE,ROOT/'tests/scripts/causal_pnl_admitted.py',ROOT/'tests/scripts/causal_pnl_reference.py']},tolerances=original['tolerances'],cases=cases)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args();data=generate();raw=(json.dumps(data,indent=2,default=c.de.serial)+'\n').encode()
 if args.write:
  if TARGET.exists():raise ValueError('Refusing to replace complete independent controls')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Independent complete boundaries differ')
 print(json.dumps(dict(status='passed',cases=len(data['cases']),positive=sum(c['expected_error'] is None for c in data['cases']),certified=False)))
