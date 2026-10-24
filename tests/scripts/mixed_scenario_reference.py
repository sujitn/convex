"""Independent complete nonlinear marking/attribution; no native execution.

Only full original supplied economics and preserved independent financial
primitives are read. Decimal60 closed cash/credit/rates/delivery formulas and
original Gaussian optional control construct every full path BEFORE admission.
"""
import argparse, copy, datetime as dt, hashlib, json, re
from decimal import Decimal as D, localcontext
from pathlib import Path
import graph_hedge_reference as g
import recovery_hedge_reference as cr
import cash_basis_hedge_reference as cb
import optional_graph_hedge_reference as opt
import delivery_hedge_reference as de
from delivery_hedge_ready import wire
ROOT=Path(__file__).resolve().parents[2];HERE=ROOT/'tests/fixtures/library'
TARGET=HERE/'mixed-scenario-reference.json'
SOURCES=['tests/fixtures/library/graph-hedge-terms.json','tests/fixtures/library/recovery-hedge-admitted.json',
 'tests/fixtures/library/cash-basis-hedge-admitted.json','tests/fixtures/library/optional-graph-hedge-resolved.json',
 'tests/fixtures/library/delivery-hedge-rebuilt.json','tests/scripts/graph_hedge_reference.py',
 'tests/scripts/recovery_hedge_reference.py','tests/scripts/cash_basis_hedge_reference.py',
 'tests/scripts/optional_graph_hedge_reference.py','tests/scripts/delivery_hedge_reference.py',
 'tests/scripts/cash_calibration_reference.py','tests/scripts/rate_calibration_reference.py',
 'tests/scripts/icma_reference.py','tests/scripts/icma_helpers_reference.py','tests/scripts/delivery_hedge_ready.py']
SNAPSHOT='mixed-scenario.explicit.synthetic.information-set.1'
clone=copy.deepcopy

def load(name):return json.loads((HERE/name).read_bytes())
def cases(name):return {c['id']:c['request'] for c in load(name)['cases']}
def all_dates(v,years):
 if isinstance(v,dict):return {k:all_dates(x,years) for k,x in v.items()}
 if isinstance(v,list):return [all_dates(x,years) for x in v]
 if isinstance(v,str) and re.match(r'^\d{4}-\d\d-\d\d(?:T|$)',v):return str(int(v[:4])+years)+v[4:]
 return v

def merge(inputs,extra):
 for name in ['calendars','curves','fixings','publications']:
  for item in extra[name]:
   if name in ['curves','calendars']:
    key=item['key' if name=='curves' else 'pin']
    old=next((i for i in inputs[name] if i['key' if name=='curves' else 'pin']==key),None)
    if old is not None:
     if old!=item:raise ValueError('Conflicting complete source '+str(key))
     continue
   if item not in inputs[name]:inputs[name].append(clone(item))

def position(id_,instrument,ref,current,pending,currency='USD',unit='notional'):
 return dict(identity=dict(id=id_,instrument=clone(instrument),economics=de.pin(id_+'.full-terms-state'),calculation=de.pin(id_+'.original-model-calculation'),entity='SCENARIO.DESK'),current=dict(quantity=str(current),reference_quantity=str(ref),quantity_unit=unit,currency=currency),pending_quantity=str(pending))
def holding(p,product,cash=None):
 h=dict(id=p['identity']['id'],product=dict(kind='product',product=product))
 if cash:h['product']=dict(kind='cash_basis',**cash)
 return h

def request(inputs,graph,positions,holdings,moves,reporting='USD',fx=None,snapshot=SNAPSHOT):
 book=dict(snapshot_id=snapshot,positions=positions,moves=moves,reporting_currency=reporting,fx=fx or [],policy=dict(max_graph_evaluations=33,max_product_evaluations=4224))
 return wire(dict(schema_version=1,inputs=inputs,operation=dict(kind='calibration',input=graph,action=dict(kind='book_scenario',input=dict(book=book,holdings=holdings)))))

def main_request(control='near_switch',reporting='USD',foreign=False):
 originals=cases('graph-hedge-terms.json');r=clone(originals['graph_hedge.positive.bond.base']);inputs=r['inputs'];graph=r['operation']['graph'];positions=[];holdings=[]
 for name,id_,current,pending in [('bond','graph_hedge.positive.bond.base','-1200','250'),('irs','graph_hedge.positive.irs.base','350000','-75000'),('ois','graph_hedge.positive.ois.reverse_side','-200000','50000'),('overnight','graph_hedge.positive.overnight_future.compounded','-2','1'),('term','graph_hedge.positive.term_future.base','3','-1')]:
  source=clone(originals[id_]);product=source['operation']['product'];h=source['operation']['input'];p=position('mixed.'+name,h['identity']['instrument'],h['reference_position']['reference_quantity'],current,pending,unit=h['reference_position']['quantity_unit']);positions.append(p);holdings.append(holding(p,product))
 recovery=cases('recovery-hedge-admitted.json')
 source=clone(recovery['recovery_hedge.book.cash']);merge(inputs,source['inputs']);op=source['operation'];h=op['input']['hedge'];p=position('mixed.credit_cash',h['identity']['instrument'],h['reference_position']['reference_quantity'],'800','-125');positions.append(p);holdings.append(holding(p,None,dict(security=op['security'],basis=op['input']['basis'],model=op['input']['model'])))
 source=clone(recovery['recovery_hedge.cds.ProtectionBuyer.positive.Pending.wire_admission']);op=source['operation'];p=position('mixed.cds',op['input']['identity']['instrument'],op['input']['reference_position']['reference_quantity'],'-450000','50000');positions.append(p);holdings.append(holding(p,op['product']))
 source=all_dates(clone(cases('optional-graph-hedge-resolved.json')['optional_graph.frn.call']),1);op=source['operation'];product=op['product'];inp=product['input'];inp['context']['settlement']='2026-10-07';inp['projection_context']['settlement']='2026-10-07';inp['projection_context']['fixing'].update(valuation_date='2026-10-07',known_at='2026-10-07T12:00:00Z');inp['discount']=dict(id='D.OIS',version='1')
 source['inputs']['curves']=source['inputs']['curves'][1:];merge(inputs,source['inputs']);p=position('mixed.optional',product['instrument'],inp['bond']['terms']['face_value'],'200','-25');positions.append(p);holdings.append(holding(p,product))
 resources,delivery=de.pack(control=control);resources['curves']=[];merge(inputs,resources);delivery['discount']['curve']=dict(id='D.OIS',version='1');delivery['discount']['stamp']=de.stamp('D.OIS',1)
 p=position('mixed.delivery',dict(id=delivery['input']['contract']['instrument_id'],version=delivery['input']['contract']['stamp']['version']),'1','-3','1',unit='contracts');positions.append(p);holdings.append(holding(p,delivery))
 moves=[dict(key=cb.factor(axis)['key'],delta=delta) for axis,delta in [('zero',.001),('unrelated',-.0015),('hazard',.003),('basis',-.0007),('recovery',.075)]]
 moves.append(dict(key=g.factor('P.SOFR')['key'],delta=.0008))
 moves.append(dict(key=opt.factor('projection',product['instrument'],inp['discount'],inp['projection'],inp['model']['key'])['key'],delta=-.001))
 moves.extend([dict(key=de.key(axis,delivery['input']),delta=delta) for axis,delta in [('repo_a',.005),('basis_a',-.015)]])
 moves.extend([dict(key=opt.factor(axis,product['instrument'],inp['discount'],inp['projection'],inp['model']['key'])['key'],delta=delta) for axis,delta in [('oas',.002),('a',-.004),('sigma',.003)]])
 fx=[]
 if foreign:
  product=clone(holdings[0]['product']['product']);product['bond']['terms']['currency']='EUR';product['method']['curve']=dict(id='D.EUR',version='1');curve=clone(inputs['curves'][0]);curve['key']=dict(id='D.EUR',version='1');curve['currency']='EUR';curve['role']['collateral_currency']='EUR';inputs['curves'].append(curve)
  p=position('mixed.eur_cash',de.pin('MIXED.EUR.CASH',2),'5000','900','-100',currency='EUR');positions.append(p);holdings.append(holding(p,product));m=clone(moves[0]);m['key']['coordinate']['curve']=dict(id='D.EUR',version='1');m['key']['factor']['target']['curve_id']='D.EUR';m['key']['factor']['currency']='EUR';m['delta']=-.001; moves.append(m)
 if reporting=='EUR':fx=[dict(from_='USD',to='EUR',rate='0.9',observation=de.pin('FX.USD.EUR.EXPLICIT',11),snapshot_id=SNAPSHOT)];fx[0]['from']=fx[0].pop('from_')
 elif foreign:fx=[dict(to='USD',rate='1.1',observation=de.pin('FX.EUR.USD.EXPLICIT',12),snapshot_id=SNAPSHOT)];fx[0]['from']='EUR'
 return request(inputs,graph,positions,holdings,moves,reporting,fx)

def state(req,moves):
 s={c['key']['id']: D(str(c['source']['Nodes'][0]['quote']['value'])) if isinstance(c['representation'],dict) or c['representation']=='IssuerSpreads' else -(D(str(c['source']['Nodes'][-1]['quote']['value'])).ln()/D(str(c['source']['Nodes'][-1]['pillar']['Tenor']))) for c in req['inputs']['curves']}
 recovery=next((D(str(c['role']['recovery'])) for c in req['inputs']['curves'] if c['role']['kind']=='Credit'),D('.4'));graph=req['operation']['input'];work={}
 rebuilt=[m for m in moves if m['key']['response']=='rebuild_dependents']
 for m in rebuilt:
  coord=m['key']['coordinate'];delta=D(str(m['delta']))
  if coord['kind']=='recovery':recovery+=delta
  elif coord['kind'] in ['zero','basis'] and coord['curve']['id'] in s:s[coord['curve']['id']]+=delta
 for r in graph['rates']:
  helper=r['recipe']['helpers'][0];q=D(str(helper['quote']['value']));key=r['recipe']['curve']['key']
  for m in rebuilt:
   coord=m['key']['coordinate']
   if coord['kind']=='quote' and coord['curve']==key and coord['helper_id']==helper['id']:q+=D(str(m['delta']))
  t=helper['terms']['terms'];T=D((de.date(t['maturity_date'])-de.date(t['start_date'])).days)/365;z=(1+q*T).ln()/T;s[key['id']]=z;work[key['id']]=dict(zero=z,quote=q)
 for r in graph['credit']:
  helper=r['helpers'][0];holidays=next(c['holidays'] for c in req['inputs']['calendars'] if c['pin']==helper['terms']['premium_calendar']);discount=s[helper['terms']['discount_curve']['id']];target=D(str(helper['quote']['value']))/10000
  fitted=cr.bisect(lambda hazard:D(cr.cds_price(helper['terms'],helper['context'],holidays,discount,hazard,recovery)['par_spread_decimal']),target,D(0),D('.1'));s[r['curve']['key']['id']]=fitted
 for r in graph['cash']:
  recipe=r['recipe'];h=recipe['helpers'][0]['input'];holidays=next(c['holidays'] for c in req['inputs']['calendars'] if c['pin']==r['calendars'][0]['pins']['payment']);projected=cb.project(h,holidays);model=recipe['basis_model'];reference_only=model['kind']=='ReferenceZeroSpread';discount=s[model['reference' if reference_only else 'discount']['id']];hazard=D(0) if reference_only else s[model['survival']['id']];target=D(cb.quote(h,projected)['present_value'])
  fitted=cr.bisect(lambda spread:D(cb.value(h,projected,discount,hazard,spread,recovery,reference_only)['present_value']),target,D('-.05'),D('.1'));s[recipe['curve']['key']['id']]=fitted
 for m in moves:
  if m['key']['response']!='frozen_curves':continue
  coord=m['key']['coordinate'];delta=D(str(m['delta']))
  if coord['kind']=='recovery':recovery+=delta
  elif coord['kind'] in ['zero','hazard','basis'] and coord['curve']['id'] in s:s[coord['curve']['id']]+=delta
 return s,recovery,work

def delivery_value(product,inputs,s,moves,baseline_s):
 p=clone(product);curve=next(c for c in inputs['curves'] if c['key']==product['discount']['curve']) if any(c['key']==product['discount']['curve'] for c in inputs['curves']) else clone(inputs.get('_oracle_curve'))
 curve=clone(curve);curve.update(representation=dict(ZeroRates=dict(compounding='Continuous')),interpolation='Linear');curve['source']=dict(Nodes=[dict(pillar=dict(Tenor=t),quote=dict(unit='DecimalRate',value=baseline_s[product['discount']['curve']['id']])) for t in [0,50]])
 for m in p['input']['members']:
  for move in moves:
   key=move['key'];typ=key['factor']['quote_type'];target=key['factor']['target']
   if key['coordinate']['kind']=='raw' and target.get('instrument_id')==m['instrument']['id']:
    if typ=='repo_rate':m['financing']['repo_decimal']=str(D(m['financing']['repo_decimal'])+D(str(move['delta'])))
    if typ=='cash_clean_price':m['clean_price_per100']=str(D(m['clean_price_per100'])+D(str(move['delta'])))
 factor=de.key('discount',p['input'],curve=product['discount']['curve']);change=s[product['discount']['curve']['id']]-baseline_s[product['discount']['curve']['id']]
 v=de.valuation(p,dict(curves=[curve]),factor,change)
 for i,member in enumerate(v['members']):member['frozen_residual_basis_per100']=D(product['input']['members'][i]['clean_price_per100'])-member['baseline']['clean']
 return dict(kind='delivery',pv=v['fair_price_points']*D(product['input']['contract']['currency_per_price_point']),full=v)

def price(req,holding,moves):
 s,recovery,work=state(req,moves);base,_,_=state(req,[]);inputs=req['inputs'];p=holding['product']
 history={(r['index_id'],dt.date.fromisoformat(r['observation_date'])):D(r['rate']) for r in inputs['fixings']}
 if p['kind']=='cash_basis':
  h=p['security']['helper'];holidays=next(c['holidays'] for c in inputs['calendars'] if c['pin']==p['security']['calendars']['payment']);projected=cb.project(h,holidays)
  model=p['model'];reference_only=model['kind']=='ReferenceZeroSpread';discount=s[model['reference' if reference_only else 'discount']['id']];hazard=D(0) if reference_only else s[model['survival']['id']]
  value=cb.value(h,projected,discount,hazard,s[p['basis']['id']],recovery,reference_only)
  return dict(kind='cash_basis',pv=D(value['present_value']),full=dict(value=value,cash=projected))
 product=p['product'];kind=product['kind']
 if kind=='bond_future':return delivery_value(product,inputs,s,moves,base)
 if kind=='optional_bond':
  inp=product['input'];oas=D(str(product['oas_decimal']));a=D(str(inp['model']['parameters']['mean_reversion']));sigma=D(str(inp['model']['parameters']['volatility']))
  for m in moves:
   typ=m['key']['factor']['quote_type']
   if typ=='oas_spread' and m['key']['factor']['target'].get('instrument_id')==product['instrument']['id']:oas+=D(str(m['delta']))
   if m['key']['factor']['target'].get('model_id')==inp['model']['key']['id']:
    if typ=='hw1f_mean_reversion':a+=D(str(m['delta']))
    if typ=='hw1f_volatility':sigma+=D(str(m['delta']))
  value=opt.financial_price(inp,inputs,discount=float(s[inp['discount']['id']]),projection=float(s[inp['projection']['id']]) if inp['projection'] else .05,oas=float(oas),a=float(a),sigma=float(sigma))
  return dict(kind='optional',pv=D(str(value['pv'])),full=value)
 if kind=='bond':
  holidays=set(next(c['holidays'] for c in inputs['calendars'] if c['pin']==product['bond']['calendars']['payment']));cash=g.cash.cash(product['bond']['terms'],product['context']['settlement'],holidays);settle=de.date(product['context']['settlement']);zero=s[product['method']['curve']['id']]+D(str(product['method']['decimal_spread']))
  pv=sum((amount*(-zero*D((de.date(on)-settle).days)/365).exp() for on,amount in cash['flows']),D(0));return dict(kind='bond',pv=pv,full=dict(cashflows=[dict(date=on,amount=amount) for on,amount in cash['flows']]))
 if kind=='cds':
  t=product['terms'];holidays=next(c['holidays'] for c in inputs['calendars'] if c['pin']==t['premium_calendar']);value=cr.cds_price(t,product['context'],holidays,s[t['discount_curve']['id']],s[t['survival_curve']['id']],recovery);return dict(kind='cds',pv=D(value['pv']),full=value)
 if kind=='swap':
  t=product['terms'];projection=t['floating']['projection_curve']['id'];value=g.swap_price(t,s[t['collateral']['discount_curve']['id']],s[projection],history);return dict(kind='swap',pv=D(value['pv']),full=value)
 if kind=='rate_future':
  inp=product['input'];value=g.future_price(inp,s[inp['projection']['curve']['id']],history);return dict(kind='rate_future',pv=D(value['pv']),full=value)
 raise ValueError('No independent full product '+kind)

def financial(req):
 data=req['operation']['action']['input'];book=data['book'];moves=book['moves'];paths=[[]]+[moves[:i] for i in range(1,len(moves)+1)]+[[m] for m in moves];trades=[]
 fx={pin['from']:D(pin['rate']) for pin in book['fx']}
 def amounts(pv,p):
  ref=D(p['current']['reference_quantity']);current=D(p['current']['quantity']);pending=D(p['pending_quantity']);return dict(current=pv*current/ref,pending=pv*pending/ref,projected=pv*(current+pending)/ref)
 def sub(a,b):return {k:a[k]-b[k] for k in a}
 def attr(values):
  baseline=values[0];scenario=values[len(moves)];total=sub(scenario,baseline);ordered=[sub(values[i],values[i-1]) for i in range(1,len(moves)+1)];standalone=[sub(v,baseline) for v in values[len(moves)+1:]]
  sums=lambda rows:{k:sum((v[k] for v in rows),D(0)) for k in baseline}
  return dict(baseline=baseline,scenario=scenario,total_change=total,ordered_effects=ordered,standalone_effects=standalone,interaction=sub(total,sums(standalone)),reconciliation_residual=sub(total,sums(ordered)))
 for p,h in zip(book['positions'],data['holdings']):
  values=[price(req,h,path) for path in paths];local=[amounts(v['pv'],p) for v in values];rate=fx.get(p['current']['currency'],D(1));reporting=[{k:v*rate for k,v in row.items()} for row in local]
  kind=values[0]['kind'];basis='FuturesQuoteLevel' if kind in ['delivery','rate_future'] else 'CurrencyPresentValue'
  epsilon=D('.006')*D(h['product']['product']['input']['bond']['terms']['face_value'])/100 if kind=='optional' else D('1e-7')
  trades.append(dict(id=p['identity']['id'],kind=kind,basis=basis,values=values,local=attr(local),reporting=attr(reporting),reference_absolute=epsilon))
 totals={};reporting={}
 for trade,p in zip(trades,book['positions']):
  for groups,key,rate in [(totals,(trade['basis'],p['current']['currency']),D(1)),(reporting,(trade['basis'],book['reporting_currency']),fx.get(p['current']['currency'],D(1)))]:
   groups.setdefault(key,[dict(current=D(0),pending=D(0),projected=D(0)) for _ in paths])
   for row,value in zip(groups[key],trade['values']):
    for k,v in amounts(value['pv'],p).items():row[k]+=v*rate
 return dict(trades=trades,local_totals=[dict(basis=k[0],currency=k[1],attribution=attr(v)) for k,v in totals.items()],reporting_totals=[dict(basis=k[0],currency=k[1],attribution=attr(v)) for k,v in reporting.items()],graph_controls=[state(req,p)[2] for p in paths])

def generate():
 cases_=[]
 def add(id_,r):cases_.append(dict(id=id_,request=r,expected_error=None,expected=financial(r)))
 with localcontext() as c:
  c.prec=60
  main=main_request();add('mixed.full.opposing',main)
  reverse=clone(main);reverse['operation']['action']['input']['book']['moves'].reverse();add('mixed.full.reverse_path',reverse)
  flat=clone(main)
  for p in flat['operation']['action']['input']['book']['positions']:p['pending_quantity']=str(-D(p['current']['quantity']))
  add('mixed.full.projected_flat',flat)
  add('mixed.full.foreign_fx',main_request(reporting='EUR',foreign=True))
  tie=main_request(control='tie');d=tie['operation']['action']['input'];d['book']['positions']=[d['book']['positions'][-1]];d['holdings']=[d['holdings'][-1]];d['book']['moves']=[d['book']['moves'][0],d['book']['moves'][8]];add('mixed.delivery.exact_tie',tie)
  zero=clone(tie)
  for m in zero['operation']['action']['input']['book']['moves']:m['delta']=0.0
  add('mixed.delivery.zero_move',zero)
  rebuilt=clone(load('delivery-hedge-rebuilt.json')['cases'][0]['request']);op=rebuilt['operation'];prod=op['action']['product'];graph=op['input'];other=clone(graph['rates'][0]);other['recipe']['curve']['key']['id']='D.AUXILIARY';other['recipe']['helpers'][0]['terms']['curve']['id']='D.AUXILIARY';other['recipe']['helpers'][0]['id']='auxiliary-full-deposit';other['recipe']['curve']['source']['Helpers']=['auxiliary-full-deposit'];graph['rates'].append(other)
  first=op['action']['key'];second=clone(first);second['coordinate']['curve']['id']='D.AUXILIARY';second['coordinate']['helper_id']='auxiliary-full-deposit';second['factor']['target']['curve_id']='D.AUXILIARY'
  p=position('mixed.rebuilt.delivery',de.pin(prod['input']['contract']['instrument_id'],4),'1','2','-1',unit='contracts');r=request(rebuilt['inputs'],graph,[p],[holding(p,prod)],[dict(key=first,delta=.004),dict(key=second,delta=-.002),dict(key=de.key('repo_a',prod['input']),delta=.003),dict(key=de.key('basis_b',prod['input']),delta=-.03)])
  # A direct oracle curve is supplied only to the independent function, never to native ingress.
  oracle=clone(graph['rates'][0]['recipe']['curve']);oracle['source']=dict(Nodes=[]);r['inputs']['_oracle_curve']=oracle;expected=financial(r);del r['inputs']['_oracle_curve'];cases_.append(dict(id='mixed.delivery.multiple_quote_rebuild',request=r,expected_error=None,expected=expected))
  historical=clone(cases('cash-basis-hedge-admitted.json')['cash_basis_hedge.rebuilt.icma']);op=historical['operation'];h=op['input']['hedge'];p=position('mixed.icma.original_history',h['identity']['instrument'],h['reference_position']['reference_quantity'],'-1200','300');r=request(historical['inputs'],op['graph'],[p],[holding(p,None,dict(security=op['security'],basis=op['input']['basis'],model=op['input']['model']))],[dict(key=f['key'],delta=delta) for f,delta in zip(h['factors'][:3],[.001,.002,-.001])],snapshot='mixed-scenario.original-icma-historical-information-set');add('mixed.icma.full_context',r)
 # Admission failures operate on full original inputs; never replace positive financial answers.
 negatives=[('empty_positions','invalid_input',lambda d:d['book'].update(positions=[])),('empty_moves','invalid_input',lambda d:d['book'].update(moves=[])),('duplicate_move','invalid_input',lambda d:d['book']['moves'].append(clone(d['book']['moves'][0]))),('missing_holding','invalid_input',lambda d:d['holdings'].pop()),('duplicate_position','invalid_input',lambda d:d['book']['positions'].append(clone(d['book']['positions'][0]))),('graph_budget','invalid_input',lambda d:d['book']['policy'].update(max_graph_evaluations=1)),('product_budget','invalid_input',lambda d:d['book']['policy'].update(max_product_evaluations=1)),('unbounded_work','invalid_input',lambda d:d['book']['policy'].update(max_product_evaluations=4225)),('fractional_pending','invalid_input',lambda d:d['book']['positions'][-1].update(pending_quantity='0.5')),('wrong_reference','invalid_input',lambda d:d['book']['positions'][-1]['current'].update(reference_quantity='2')),('wrong_delivery_revision','invalid_input',lambda d:d['book']['positions'][-1]['identity']['instrument'].update(version=99)),('raw_repo_revision','invalid_input',lambda d:d['book']['moves'][7]['key']['coordinate'].update(version=99)),('bad_recovery','invalid_input',lambda d:d['book']['moves'][4].update(delta=.7)),('missing_factor_curve','missing_input',lambda d:d['book']['moves'][0]['key']['coordinate']['curve'].update(version='missing')),('optional_shaped','unsupported',lambda d:d['book']['moves'][0]['key']['coordinate'].update(node=1)),('optional_negative_sigma','invalid_input',lambda d:d['book']['moves'][-1].update(delta=-.04)),('optional_model_revision','invalid_input',lambda d:d['book']['moves'][-1]['key']['factor']['target'].update(model_version='foreign')),('changed_snapshot_fx','invalid_input',lambda d:d['book'].update(reporting_currency='EUR',fx=[{'from':'USD','to':'EUR','rate':'0.9','observation':de.pin('FX.EXPLICIT',1),'snapshot_id':'different-information-set'}])),('missing_fx','invalid_input',lambda d:d['book'].update(reporting_currency='EUR')),('extra_fx','invalid_input',lambda d:d['book'].update(fx=[{'from':'EUR','to':'USD','rate':'1.1','observation':de.pin('FX.EXTRA',1),'snapshot_id':SNAPSHOT}]))]
 for label,error,change in negatives:
  r=clone(main);change(r['operation']['action']['input']);cases_.append(dict(id='mixed.invalid.'+label,request=r,expected_error=error))
 return dict(certified=False,reference='Independent Decimal60 complete cash/credit/rates/carry plus original closed Gaussian optional payoff, never native output.',source_reads={p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in SOURCES},tolerances=dict(money='1e-7',delivery_money='1e-5',delivery_per100='1e-8',cash='1e-10',rate='1e-12',optional_price_per100='.006',attribution='Triangle bounds from independently checked monetary levels; original native convergence/stability defaults unchanged.'),cases=cases_)

if __name__=='__main__':
 p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();value=generate();raw=(json.dumps(value,indent=2,default=de.serial)+'\n').encode()
 if a.write:
  if TARGET.exists():raise ValueError('Refusing to overwrite original financial declaration')
  TARGET.write_bytes(raw)
 elif TARGET.read_bytes()!=raw:raise ValueError('Original full nonlinear financial controls changed')
 print(json.dumps(dict(status='passed',cases=len(value['cases']),successes=sum(c['expected_error'] is None for c in value['cases']),certified=False)))
