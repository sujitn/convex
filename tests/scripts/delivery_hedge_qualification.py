"""Separately predeclared full smaller-bump CTD-switch controls; no wider bound."""
import argparse
from decimal import Decimal, localcontext
import json
from delivery_hedge_reference import ROOT, HERE, serial, generate, answer, settings

TARGET=HERE/'delivery-hedge-qualified.json'

def qualified():
    raw=generate();cases=[]
    with localcontext() as c:
        c.prec=60
        for original in raw['cases']:
            if original['expected_error'] is None: continue
            case=json.loads(json.dumps(original,default=serial));action=case['request']['operation']['action']
            case['id']+='.resolved_bump';action['settings']=settings('0.000005')
            case['expected']=answer(action['product'],case['request']['inputs'],action['key'],action['settings'],
                Decimal(action['position']['quantity']))
            expected=case['expected'];p=action['settings']['stability']
            bound=Decimal(str(p['absolute']))+Decimal(str(p['relative']))*max(abs(expected['value']),abs(expected['half_value']))
            if abs(expected['value']-expected['half_value'])>bound: raise ValueError('Declared narrower switch bump still unresolved')
            case.update(expected_error=None,independent_stability_passed=True);cases.append(case)
    return dict(qualification='Separate 0.000005 original canonical bumps; original h=0.0002 controls remain, all financial formulas and absolute/relative policies unchanged.',
        bounds=raw['bounds'],source_reads=raw['source_reads'],cases=cases,certified=False)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args()
    raw=(json.dumps(qualified(),indent=2,default=serial)+'\n').encode()
    if a.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite independent switch admission')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Independent switch controls differ')
    print(json.dumps(dict(status='passed',qualified_switches=len(qualified()['cases']),certified=False)))
