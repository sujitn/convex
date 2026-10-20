"""Separately declared interacting-lot and conditional-continuous Decimal controls."""
from copy import deepcopy
from decimal import Decimal as D
import argparse
import itertools
import json
from pathlib import Path
from constrained_hedge_reference import problem, FACTORS, calculate
FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'

def generate():
    cases=[];references=[]
    def add(name,p,continuous=None,proposal=None,status='feasible',action='solve',error=None):
        # Original CurveKey has an opaque STRING revision; native Version is separate.
        for f in p['factors']:f['key']['coordinate']['curve']['version']=str(f['key']['coordinate']['curve']['version'])
        for v in [h['risk'] for h in p['holdings']]+[c['risk'] for c in p['candidates']]:
            for s in v['sensitivities']:s['key']['coordinate']['curve']['version']=str(s['key']['coordinate']['curve']['version'])
        case={'id':'hedge.'+name,'expected_error':error,'expected_status':status if action=='solve' and error is None else None,
          'request':{'schema_version':1,'inputs':{'calendars':[],'helper_calendars':[],'curves':[],'calibration':None,'fixings':[],'publications':[]},
          'operation':{'kind':'hedge','problem':p,'action':{'kind':action}}}};cases.append(case)
        if continuous is not None:
            expected=calculate(p,continuous)
            if proposal is not None:expected['proposal']=calculate(p,proposal)['proposal']
            if action=='solve':assert expected['continuous'] and expected['proposal']
            references.append({'id':case['id'],'expected':expected,'absolute':1e-8,'quantity_absolute':1e-7,'relative':0.0})
    two=[deepcopy(FACTORS[0]),deepcopy(FACTORS[2])]
    p=problem([1,0],[[1,1],[1,-1]],['cash_bond','irs'],two)
    for c in p['candidates']:c['lot']='1000'
    p['factors'][0]['lower']=p['factors'][0]['upper']=0.0;p['factors'][1]['lower']=-1.0;p['factors'][1]['upper']=1.0
    # Exact Decimal finite lattice: independent objective/constraint arithmetic.
    best=None
    for x,y in itertools.product(range(-10,11),repeat=2):
        value=calculate(p,[x,y])['proposal']
        if value and (best is None or D(value['objective'])<D(best['objective'])):best=value
    assert best['reference_multiples']==['-1','0']
    add('interacting_lots',p,[D('-.5'),D('-.5')],[-1,0])
    p=problem([D('.3'),D('1.4')],[[1,0],[1,1]],['cash_bond','futures'],two)
    # For each ORIGINAL integer futures quantity, independently eliminate cash
    # via d(objective)/d(x_cash)=0. No candidate output or Rust solve consumed.
    best=None
    for y in range(-10,11):
        x=-D('.3')-D(y);value=calculate(p,[x,y])['proposal']
        if value and (best is None or D(value['objective'])<D(best['objective'])):best=value
    assert best['reference_multiples']==['0.7','-1']
    add('conditional_continuous',p,[D('1.1'),D('-1.4')],[D('.7'),-1])
    p=problem([1,0],[[1,1],[1,-1]],['cash_bond','irs'],two)
    p['candidates'][0].update(lot='1',trade_lower='-1000000000000000',trade_upper='1000000000000000',
       holding_lower='-1000000000000000',holding_upper='1000000000000000',buy_available='1000000000000000',sell_available='1000000000000000')
    p['candidates'][1].update(lot='1000',trade_lower='100',trade_upper='200')
    add('empty_later_lot',p,status='discrete_infeasible')
    p=problem([1],[[1]],['cash_bond'],[FACTORS[0]])
    add('aggregate_boundary',p,[-1],action='aggregate')
    p=problem([1],[[1]],['cash_bond'],[FACTORS[0]]);p['holdings']=[]
    add('aggregate_missing',p,error='missing_input',action='aggregate')
    return {'certified':False,'provenance':'Separately declared original Decimal60 interacting/conditional native-lot and bounded-empty-axis controls, before this corpus execution.','cases':cases}, {'certified':False,'precision':60,'cases':references}

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    controls,reference=generate()
    for name,value in [('constrained-hedge-lots.json',controls),('constrained-hedge-lots-reference.json',reference)]:
        path=FOLDER/name;content=json.dumps(value,indent=2)+'\n'
        if args.write:
            if path.exists():raise SystemExit('Refusing to overwrite original '+name)
            path.write_text(content)
        elif path.read_text()!=content:raise ValueError('Frozen original control differs: '+name)
    print(json.dumps({'requests':len(controls['cases']),'independent_controls':len(reference['cases']),'certified':False}))
