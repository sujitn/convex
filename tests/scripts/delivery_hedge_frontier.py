"""Non-tied original CTD switches, unresolved risk and single-member boundaries."""
import argparse,copy,json
from decimal import Decimal,localcontext
from delivery_hedge_reference import HERE,D,serial,pack,key,settings,request,valuation,answer
from delivery_hedge_ready import wire
TARGET=HERE/'delivery-hedge-frontier.json'

def generate():
    cases=[]
    with localcontext() as c:
        c.prec=60
        for axis in ['discount','basis_a']:
            resources,product=pack(control='near_switch');factor=key(axis,product['input']);policy=settings('.01' if axis.startswith('basis') else '.0002')
            expected=answer(product,resources,factor,policy);bound=D(str(policy['stability']['absolute']))+D(str(policy['stability']['relative']))*max(abs(expected['value']),abs(expected['half_value']))
            if abs(expected['value']-expected['half_value'])<=bound: raise ValueError('Required independently unresolved near-switch control unexpectedly stable')
            cases.append(dict(id='delivery.frontier.unresolved.'+axis,request=wire(request(resources,product,factor,policy)),expected_error='invalid_input',expected=expected))
            for move in ['.001','-.001'] if axis=='discount' else ['.05','-.05']:
                r=request(resources,product,factor,policy,-3);a=r['operation']['action'];a.update(kind='product_scenario',delta=float(move));del a['settings']
                base=valuation(product,resources,factor,D(0));shock=valuation(product,resources,factor,D(move))
                if len(base['fair_ties'])!=1: raise ValueError('Original frontier must have a unique positive-gap baseline')
                cases.append(dict(id='delivery.frontier.scenario.'+axis+'.'+move,request=wire(r),expected_error=None,expected=dict(baseline=base,shocked=shock)))
        resources,product=pack(control='stable');product['input']['members']=product['input']['members'][:1]
        product['input']['basket']['bonds']=product['input']['basket']['bonds'][:1]
        factor=key('discount',product['input']);policy=settings()
        cases.append(dict(id='delivery.boundary.single_member',request=wire(request(resources,product,factor,policy)),expected_error=None,expected=answer(product,resources,factor,policy)))
    return dict(certified=False,qualification='Original unique positive-gap baseline switches under actual full-member scenarios. Unresolved h/h2 secants remain rejected at unchanged bounds, all full independent five-price answers retained. Single-member full risk retains absent runner-up gap.',cases=cases)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=generate();raw=(json.dumps(v,indent=2,default=serial)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite independent frontier controls')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Full original frontier controls differ')
    print(json.dumps(dict(status='passed',controls=len(v['cases']),certified=False)))
