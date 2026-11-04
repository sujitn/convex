#!/usr/bin/env python3
"""Compare complete actual bill results with prior independent Decimal60 answers."""
import argparse
from decimal import Decimal as D, localcontext
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[2]


def compare(actual, expected, absolute, relative, path=''):
    checks=0
    for key,value in expected.items():
        label=path+'.'+key
        if isinstance(value,dict):
            checks+=compare(actual[key],value,absolute,relative,label)
        elif isinstance(value,str) and key not in ['method']:
            if not isinstance(actual[key],str): raise ValueError('Nonexact decimal result '+label)
            a,e=D(actual[key]),D(value)
            if abs(a-e)>absolute+relative*abs(e): raise ValueError(f'{label}: {a} != {e}')
            checks+=1
        elif actual[key]!=value: raise ValueError('Convention/date branch changed '+label)
    return checks


def verify(data):
    source=json.loads((ROOT/'tests/fixtures/library/treasury-bill-admitted.json').read_bytes())
    reference=json.loads((ROOT/'tests/fixtures/library/treasury-bill-reference-admitted.json').read_bytes())
    if data['certified'] is not False: raise ValueError('Transport cannot certify finance')
    fields=lambda c: (c['id'],c['request'],c['expected_error'])
    if [fields(c) for c in data['cases']]!=[fields(c) for c in source['cases']]:
        raise ValueError('Complete original corpus changed')
    actual={}
    for c in data['cases']:
        out=c['expected_envelope']
        if c['expected_error'] is not None:
            if out['ok']!='false' or out['error']['code']!=c['expected_error'] or 'result' in out:
                raise ValueError('Typed negative changed '+c['id'])
            continue
        if out['ok']!='true': raise ValueError('Expected complete positive '+c['id'])
        result=out['result']['data']
        if result['terms']!=c['request']['operation']['input']:
            raise ValueError('Original economics/calendar/metadata lost '+c['id'])
        if result['calculation']['quote']!=c['request']['operation']['action']['quote']:
            raise ValueError('Original quote changed '+c['id'])
        actual[c['id']]=result['calculation']
    if set(actual)!={c['id'] for c in reference['cases']}: raise ValueError('Missing independent answer')
    checks=0
    with localcontext() as ctx:
        ctx.prec=60
        for c in reference['cases']:
            a=actual[c['id']]
            checks+=compare(a,c['expected'],D(reference['absolute_tolerance']),D(reference['relative_tolerance']),c['id'])
            if abs(D(a['native_price_residual_per100']))>D('1e-18'):
                raise ValueError('Native inverse residual failed '+c['id'])
        for c in reference['convergence']:
            a=actual[c['id']]; p=D(a['price_per100']); risk=a['investment_risk']; errors=[]
            for name,h in [('h',D('.0001')),('h2',D('.00005'))]:
                up=D(actual[c['inputs'][name+'.up']]['price_per100'])
                down=D(actual[c['inputs'][name+'.down']]['price_per100'])
                first=(up-down)/(2*h); second=(up-2*p+down)/(h*h)
                ideal_first=D(risk['d_price_per100_d_decimal_rate'])
                ideal_second=D(risk['d2_price_per100_d_decimal_rate2'])
                e1,e2=abs(first-ideal_first),abs(second-ideal_second)
                if e1>D('1e-18')+abs(ideal_first)*D(reference['finite_difference_first_relative']):
                    raise ValueError('First sensitivity convergence failed '+c['id'])
                if e2>D('1e-14')+abs(ideal_second)*D(reference['finite_difference_second_relative']):
                    raise ValueError('Second sensitivity convergence failed '+c['id'])
                errors.append((e1,e2));checks+=2
            # Independent halving must improve both derivatives, except the
            # explicitly declared arithmetic floor; no model resolution bypass.
            if errors[1][0]>errors[0][0]/2+D('1e-18') or errors[1][1]>errors[0][1]/2+D('1e-14'):
                raise ValueError('h/2 failed to refine sensitivity '+c['id'])
    return dict(status='passed',complete_requests=len(data['cases']),positive_answers=len(actual),
                typed_failures=len(data['cases'])-len(actual),numeric_checks=checks,
                convergence_cases=len(reference['convergence']),certified=False)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    source=parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--binary');source.add_argument('--parity')
    args=parser.parse_args()
    raw=subprocess.check_output([args.binary,'--emit-parity']) if args.binary else Path(args.parity).read_bytes()
    print(json.dumps(verify(json.loads(raw))))
