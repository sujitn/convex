#!/usr/bin/env python3
"""Full supplied UFR contracts; financial expected values remain separate."""
import argparse
import copy
import json
from pathlib import Path

from library_curve_family_inputs import key, recipe, request

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT/'tests/fixtures/library/ufr.json'


def recipes(model):
    identity = 'ufr.'+model['id']
    if model['family'] == 'parametric':
        p=model['parameters']
        base=recipe('base.'+model['id'],dict(Parametric=dict(parameters=p,horizon=dict(Tenor=150.0),observations=[])),
                    'NelsonSiegel' if p['kind']=='nelson_siegel' else 'Svensson')
    else:
        nodes=[]
        for i,(t,z) in enumerate(zip(model['knots'],model['zeros'])):
            pillar=dict(Date=model['dates'][i]) if 'dates' in model else dict(Tenor=float(t))
            nodes.append(dict(pillar=pillar,quote=dict(value=float(z)*(100 if i%2 else 1),
                         unit='PercentRate' if i%2 else 'DecimalRate')))
        base=recipe('base.'+model['id'],dict(FlatForward=dict(nodes=nodes)),'FlatForward')
    base['time_basis']=model.get('time_basis','Act365Fixed')
    llp=dict(Date=model['llp_date']) if 'llp_date' in model else dict(Tenor=float(model['llp']))
    tail=copy.deepcopy(base);tail['key']=key(identity);tail['dependencies']=[copy.deepcopy(base['key'])]
    tail['source']=dict(UfrConvergence=dict(base=copy.deepcopy(base['key']),last_liquid_point=llp))
    tail['extrapolation']=dict(UfrConvergence=dict(ufr=float(model['ufr']),alpha=float(model['alpha'])))
    return [tail,base]


def generate():
    reference=json.loads((ROOT/'tests/fixtures/library/ufr-reference.json').read_bytes())
    cases=[]
    def add(identity,q,error=None):
        cases.append(dict(id=identity,request=copy.deepcopy(q),expected_error=error));return q
    def query(identity,curves,kind,point,error=None):
        return add(identity,request(curves,dict(kind='curve_query',curve=curves[0]['key'],
                   query=dict(kind=kind,point=point))),error)
    for model in reference['models']+reference['dated']:
        curves=recipes(model);identity=model['id']
        add('ufr.describe.'+identity,request(curves,dict(kind='curves')))
        add('ufr.roundtrip.'+identity,request(curves,dict(kind='roundtrip')))
        for point in model['points']:
            coordinate=dict(Date=point['date']) if 'date' in point else dict(Tenor=float(point['time']))
            for kind in ['Raw','DiscountFactor','UfrForward']:
                query('ufr.'+kind+'.'+identity+'.'+point['time'],curves,kind,coordinate)
        for kind in ['Raw','DiscountFactor']:
            query('ufr.source.'+kind+'.'+identity,curves,kind,dict(Tenor=float(model['llp'])/2))
        for kind in ['UfrForward','InstantaneousForward','FlatForwardRate','HazardRate']:
            q=request(curves,dict(kind='curve_query',curve=curves[0]['key'],query=dict(kind=kind,point=dict(Tenor=1.0))))
            if kind in ['FlatForwardRate','HazardRate']:q['operation']['query']['side']='Left'
            add('ufr.failure.other_query.'+kind+'.'+identity,q,'invalid_input' if kind=='UfrForward' and float(model['llp'])>1 else 'unsupported')
            # Tiny LLP legitimately admits time 1 on its UFR tail.
            if kind=='UfrForward' and float(model['llp'])<=1:cases[-1]['expected_error']=None
        query('ufr.failure.ceiling.'+identity,curves,'Raw',dict(Tenor=151.0),'invalid_input')
    seed=recipes(reference['models'][0])
    query('ufr.origin.identity',seed,'DiscountFactor',dict(Tenor=0.0))
    query('ufr.origin.raw',seed,'Raw',dict(Tenor=0.0))
    query('ufr.failure.before_llp',seed,'UfrForward',dict(Tenor=19.0),'invalid_input')
    query('ufr.failure.negative_time',seed,'Raw',dict(Tenor=-1.0),'invalid_input')
    for field,value,error in [
        ('calibration','Piecewise','unsupported'),('extrapolation','None','invalid_input'),
        ('extrapolation','FlatForward','invalid_input'),('interpolation','Linear','invalid_input'),
        ('representation',dict(ZeroRates=dict(compounding='SemiAnnual')),'unsupported'),
        ('representation','DiscountFactors','unsupported'),('currency','EUR','invalid_input'),
        ('reference_date','2024-03-01','invalid_input'),('time_basis','Act360','invalid_input'),
        ('time_basis','ActActIcma','unsupported'),('dependencies',[],'invalid_input'),
        ('role',dict(kind='Government',benchmark_id='government.v1'),'invalid_input'),
        ('role',dict(kind='Credit',entity_id='entity.v1',seniority='seniority.v1',recovery=0.4),'invalid_input'),
    ]:
        bad=copy.deepcopy(seed);bad[0][field]=value
        add('ufr.failure.'+field+'.'+str(len(cases)),request(bad,dict(kind='curves')),error)
    for name,values in [
        ('alpha_zero',dict(ufr=0.042,alpha=0.0)),('alpha_negative',dict(ufr=0.042,alpha=-0.1)),
        ('ufr_rate_bound',dict(ufr=10.00001,alpha=0.1)),
    ]:
        bad=copy.deepcopy(seed);bad[0]['extrapolation']=dict(UfrConvergence=values)
        add('ufr.failure.'+name,request(bad,dict(kind='curves')),'invalid_input')
    for t in [0.0,151.0,51.0]:
        bad=copy.deepcopy(seed);bad[0]['source']['UfrConvergence']['last_liquid_point']=dict(Tenor=t)
        add('ufr.failure.llp.'+str(t),request(bad,dict(kind='curves')),'invalid_input')
    missing=copy.deepcopy(seed);missing[0]['source']['UfrConvergence']['base']['version']='absent.v1'
    missing[0]['dependencies']=[missing[0]['source']['UfrConvergence']['base']]
    add('ufr.failure.missing_version',request(missing,dict(kind='curves')),'missing_input')
    add('ufr.failure.missing_base',request(seed[:1],dict(kind='curves')),'missing_input')
    cycle=copy.deepcopy(seed);cycle[0]['source']['UfrConvergence']['base']=cycle[0]['key'];cycle[0]['dependencies']=[cycle[0]['key']]
    add('ufr.failure.cycle',request(cycle,dict(kind='curves')),'invalid_input')
    for source,method in [(dict(Nodes=seed[1]['source']['FlatForward']['nodes']),'Linear'),
                          (dict(PiecewiseConstant=dict(nodes=seed[1]['source']['FlatForward']['nodes'])),'PiecewiseConstant')]:
        bad=copy.deepcopy(seed);bad[1]['source']=source;bad[1]['interpolation']=method;bad[0]['interpolation']=method
        add('ufr.failure.boundary_model.'+method,request(bad,dict(kind='curves')),'unsupported')
    for role in [dict(kind='Projection',index_id='index.v1'),dict(kind='Government',benchmark_id='government.v1'),
                 dict(kind='Repo',collateral_id='bond.v1')]:
        changed=copy.deepcopy(seed)
        for c in changed:c['role']=role
        add('ufr.role.'+role['kind'],request(changed,dict(kind='curves')))
    short=copy.deepcopy(seed);short[1]['source']['FlatForward']['nodes'].pop(0);short[1]['extrapolation']='FlatForward'
    query('ufr.failure.no_short_extension',short,'Raw',dict(Tenor=5.0),'invalid_input')
    query('ufr.positive_origin_identity',short,'DiscountFactor',dict(Tenor=0.0))
    bad=copy.deepcopy(short);bad[0]['source']['UfrConvergence']['last_liquid_point']=dict(Tenor=10.0)
    add('ufr.failure.source_extension_at_llp',request(bad,dict(kind='curves')),'invalid_input')
    # Existing views consume the original tail exactly, without a sampled grid.
    equivalent=copy.deepcopy(seed[0]);equivalent['key']=key('ufr.equivalent');equivalent['dependencies']=[seed[0]['key']]
    equivalent['source']=dict(DiscountEquivalent=dict(base=seed[0]['key']));equivalent['representation']='DiscountFactors'
    query('ufr.view.exact_discount',[equivalent]+seed,'Raw',dict(Tenor=60.0))
    for kind in ['Raw','UfrForward']:
        base=seed[1];q=request([base],dict(kind='curve_query',curve=base['key'],query=dict(kind=kind,point=dict(Tenor=20.0))))
        add('ufr.base.'+kind,q,'unsupported' if kind=='UfrForward' else None)
    # Strict original generic node contract remains unsupported for UFR.
    bare=copy.deepcopy(seed[1]);bare['source']=dict(Nodes=bare['source']['FlatForward']['nodes']);bare['interpolation']='Linear'
    bare['extrapolation']=seed[0]['extrapolation']
    add('ufr.failure.bare_policy',request([bare],dict(kind='curves')),'unsupported')
    return dict(schema_version=1,certified=False,
        qualification='Full original supplied UFR engineering contracts; fresh native envelopes are transport expectations, independent frozen financial controls are separate.',cases=cases)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--check',action='store_true');args=p.parse_args()
    data=generate();raw=(json.dumps(data,sort_keys=True,separators=(',',':'))+'\n').encode()
    if args.check:
        if OUTPUT.read_bytes()!=raw:raise SystemExit('Full UFR input corpus differs')
    else:OUTPUT.write_bytes(raw)
    print(json.dumps(dict(status='passed',cases=len(data['cases']),successes=sum(c['expected_error'] is None for c in data['cases']))))


if __name__=='__main__':main()
