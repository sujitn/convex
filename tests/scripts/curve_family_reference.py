#!/usr/bin/env python3
"""Independent Decimal(60) curve-family controls, frozen before native execution."""
import argparse
import datetime as dt
import json
from decimal import Decimal, localcontext
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / 'tests/fixtures/library/curve-family-reference.json'
D = Decimal


def zero(p, t):
    t = D(str(t))
    b = [D(str(p[k])) for k in ['beta0', 'beta1', 'beta2']]
    if t == 0:
        return b[0] + b[1]
    x = t / D(str(p.get('tau', p.get('tau1'))))
    e = (-x).exp()
    l1 = (1 - e) / x
    z = b[0] + b[1] * l1 + b[2] * (l1 - e)
    if p['kind'] == 'svensson':
        x2 = t / D(str(p['tau2']))
        e2 = (-x2).exp()
        z += D(str(p['beta3'])) * ((1 - e2) / x2 - e2)
    return z


def forward(p, t):
    t = D(str(t))
    x = t / D(str(p.get('tau', p.get('tau1'))))
    e = (-x).exp()
    f = D(str(p['beta0'])) + D(str(p['beta1'])) * e + D(str(p['beta2'])) * x * e
    if p['kind'] == 'svensson':
        x2 = t / D(str(p['tau2']))
        f += D(str(p['beta3'])) * x2 * (-x2).exp()
    return f


def generate():
    with localcontext() as context:
        context.prec = 60
        models = [
            dict(id='ns.positive', kind='nelson_siegel', beta0=0.045, beta1=-0.02, beta2=0.01, tau=2.0),
            dict(id='ns.negative', kind='nelson_siegel', beta0=-0.004, beta1=-0.003, beta2=0.002, tau=1.5),
            dict(id='sv.positive', kind='svensson', beta0=0.045, beta1=-0.02, beta2=0.01, beta3=-0.005, tau1=2.0, tau2=8.0),
            dict(id='sv.negative', kind='svensson', beta0=-0.004, beta1=-0.003, beta2=0.002, beta3=-0.001, tau1=1.5, tau2=6.0),
        ]
        reference = dt.date(2024, 2, 29)
        cases = []
        for model in models:
            for t in ['0', '0.000000000001', '0.25', '0.5', '1', '2', '5', '8', '10', '30', '150']:
                time = D(t)
                z = zero(model, time)
                cases.append(dict(id=model['id'] + '.' + t, parameters={k:v for k,v in model.items() if k!='id'}, tenor=t,
                                  zero=str(z), discount=str((-z*time).exp()), instantaneous_forward=str(forward(model,time))))
        observations = {}
        for model in models:
            observations[model['id']] = []
            for i, days in enumerate([30,90,180,365,730,1095,1825,3650,7300,10950]):
                date = reference + dt.timedelta(days=days)
                t = D(days)/D(365)
                observations[model['id']].append(dict(id='zero.'+str(i),version='quote.'+str(i+7),pillar=str(date),
                    quote=dict(value=float(zero(model,t)*D(100) if i%2 else zero(model,t)),unit='PercentRate' if i%2 else 'DecimalRate'),tolerance=1e-10))
        # Independent full curves used by native segmented/delegated owners.
        # Adjacent zero segments join at z(2)=0.04; the right segment owns t=2.
        structure = []
        for t in ['0','1','1.999999','2','2.000001','3','4','5']:
            time=D(t)
            z=D('0.03')+D('0.005')*time if time<D(2) else D('0.04')-D('0.0025')*(time-D(2))
            structure.append(dict(tenor=t,zero=str(z),discount=str((-z*time).exp())))
        delegated = []
        for t in ['0','0.5','1','3','4','5','150']:
            time=D(t); effective=min(max(time,D(1)),D(4))
            z=D('0.03')+D('0.005')*effective
            delegated.append(dict(tenor=t,zero=str(z),discount=str((-z*time).exp()),extrapolated=time<D(1) or time>D(4)))
        return dict(schema_version=1,certified=False,provenance='Independent synthetic Decimal(60) analytic zero/discount/forward controls; original NS/Svensson equations and exact dated ACT/365F. No native/comparator outputs used.',
                    reference_date=str(reference),bounds=dict(rate_absolute=1e-10,discount_absolute=3e-10,fit_beta_absolute=1e-6,fit_tau_absolute=1e-3),
                    models=models,cases=cases,observations=observations,segments=structure,delegated=delegated,
                    fit_settings=dict(lower=[-0.1,-0.1,-0.1,0.1],upper=[0.1,0.1,0.1,10.0],quote_scales=[0.0001 if i%2==0 else 0.01 for i in range(10)],weights=[1.0]*10,
                        max_iterations=100,objective_tolerance=1e-8,controls=dict(jacobian_step=1e-6,minimum_singular_ratio=1e-10,svd_iterations=1000)))


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode',choices=['write','check']);args=parser.parse_args()
    data=generate();raw=(json.dumps(data,indent=2,sort_keys=True)+'\n').encode()
    if args.mode=='write': OUTPUT.write_bytes(raw)
    elif OUTPUT.read_bytes()!=raw: raise ValueError('Predeclared independent financial controls changed')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),observations=sum(map(len,data['observations'].values())),certified=False)))


if __name__=='__main__': main()
