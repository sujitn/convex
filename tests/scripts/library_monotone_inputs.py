#!/usr/bin/env python3
"""Full supplied MonotoneConvex corpus; financial values remain independent."""
import argparse
import copy
import json
from pathlib import Path
from library_curve_family_inputs import key, recipe, request

ROOT=Path(__file__).resolve().parents[2]
OUTPUT=ROOT/'tests/fixtures/library/monotone.json'


def source(model, tail='FlatForward'):
    nodes=[]
    for i,(t,z) in enumerate(zip(model['knots'],model['zeros'])):
        point=dict(Date=model['dates'][i]) if 'dates' in model else dict(Tenor=float(t))
        nodes.append(dict(pillar=point,quote=dict(value=float(z)*(100 if i%2 else 1),unit='PercentRate' if i%2 else 'DecimalRate')))
    curve=recipe('monotone.'+model['id'],dict(MonotoneConvex=dict(nodes=nodes)),'MonotoneConvex',tail)
    curve['time_basis']=model['time_basis']
    return curve


def generate():
    ref=json.loads((ROOT/'tests/fixtures/library/monotone-reference.json').read_bytes());cases=[]
    def add(identity,q,error=None):
        cases.append(dict(id=identity,request=copy.deepcopy(q),expected_error=error))
    def query(identity,c,kind,point,side=None,error=None):
        q=dict(kind=kind,point=point)
        if side:q['side']=side
        add(identity,request([c],dict(kind='curve_query',curve=c['key'],query=q)),error)
    for m in ref['models']+ref['dated']:
        c=source(m);identity=m['id'];last=float(m['knots'][-1])
        last_point=dict(Date=m['dates'][-1]) if 'dates' in m else dict(Tenor=last)
        add('monotone.describe.'+identity,request([c],dict(kind='curves')))
        add('monotone.roundtrip.'+identity,request([c],dict(kind='roundtrip')))
        for p in m['points']+m.get('dated_points',[]):
            point=dict(Date=p['date']) if 'date' in p else dict(Tenor=float(p['time']))
            suffix=p.get('date',p['time'])
            for kind in ['Raw','DiscountFactor','MonotoneConvexForward']:
                query('monotone.'+kind+'.'+identity+'.'+suffix,c,kind,point,'Right' if kind=='MonotoneConvexForward' else None)
        for kind in ['UfrForward','FlatForwardRate','InstantaneousForward','HazardRate']:
            query('monotone.failure.other_query.'+identity+'.'+kind,c,kind,dict(Tenor=1.0),'Left' if kind in ['FlatForwardRate','HazardRate'] else None,'unsupported')
        query('monotone.failure.origin_left.'+identity,c,'MonotoneConvexForward',dict(Tenor=0.0),'Left','invalid_input')
        query('monotone.last_left.'+identity,c,'MonotoneConvexForward',last_point,'Left')
        for tail in ['None','Flat']:
            changed=copy.deepcopy(c);changed['extrapolation']=tail
            query('monotone.last_right.'+tail+'.'+identity,changed,'MonotoneConvexForward',last_point,'Right','invalid_input' if tail=='None' else None)
            query('monotone.tail.'+tail+'.'+identity,changed,'Raw',dict(Tenor=150.0),error='invalid_input' if tail=='None' else None)
        add('monotone.zero_rate.'+identity,request([c],dict(kind='curve_query',curve=c['key'],query=dict(kind='ZeroRate',point=dict(Tenor=1.0),day_count=c['time_basis'],compounding='Annual'))))
        for kind in ['DiscountRatio','ForwardRate']:
            q=dict(kind=kind,start=dict(Tenor=.25),end=dict(Tenor=last))
            if kind=='ForwardRate':q.update(day_count=c['time_basis'],compounding='Continuous')
            add('monotone.period.'+kind+'.'+identity,request([c],dict(kind='curve_query',curve=c['key'],query=q)))
    seed=source(ref['models'][1])
    for field,value,error in [
        ('calibration','Piecewise','unsupported'),('interpolation','Linear','invalid_input'),
        ('extrapolation','Linear','unsupported'),('extrapolation',dict(UfrConvergence=dict(ufr=.04,alpha=.1)),'unsupported'),
        ('representation','DiscountFactors','unsupported'),('representation',dict(ZeroRates=dict(compounding='Annual')),'unsupported'),
        ('time_basis','ActActIcma','unsupported'),('dependencies',[key('absent')],'invalid_input'),
        ('role',dict(kind='Discount',collateral_currency='EUR'),'invalid_input'),
        ('role',dict(kind='Credit',entity_id='e.v1',seniority='seniority.v1',recovery=.4),'invalid_input'),
    ]:
        c=copy.deepcopy(seed);c[field]=value
        add('monotone.failure.'+field+'.'+str(len(cases)),request([c],dict(kind='curves')),error)
    for name,edit,error in [
        ('negative_first',lambda n:n[0]['quote'].update(value=-.01),'invalid_input'),
        ('negative_interval',lambda n:n[1]['quote'].update(value=0.0),'invalid_input'),
        ('duplicate',lambda n:n[1].update(pillar=n[0]['pillar']),'invalid_input'),
        ('zero_pillar',lambda n:n[0].update(pillar=dict(Tenor=0.0)),'invalid_input'),
        ('one_node',lambda n:n.__delitem__(slice(1,None)),'invalid_input'),
        ('quote_units',lambda n:n[0]['quote'].update(unit='DiscountFactor'),'invalid_input'),
        ('rate_bound',lambda n:n[0]['quote'].update(value=10.001),'invalid_input'),
    ]:
        c=copy.deepcopy(seed);edit(c['source']['MonotoneConvex']['nodes'])
        add('monotone.failure.'+name,request([c],dict(kind='curves')),error)
    for t in [-1.0,151.0]:query('monotone.failure.time.'+str(t),seed,'Raw',dict(Tenor=t),error='invalid_input')
    reversed_source=copy.deepcopy(seed);reversed_source['source']['MonotoneConvex']['nodes'].reverse()
    query('monotone.permuted_nodes',reversed_source,'Raw',dict(Tenor=3.0))
    generic=copy.deepcopy(seed);generic['source']=dict(Nodes=generic['source']['MonotoneConvex']['nodes'])
    add('monotone.failure.generic_nodes',request([generic],dict(kind='curves')),'unsupported')
    wrong=copy.deepcopy(seed);wrong['source']=dict(FlatForward=wrong['source']['MonotoneConvex']);wrong['interpolation']='FlatForward'
    query('monotone.failure.other_source',wrong,'MonotoneConvexForward',dict(Tenor=1.0),'Right','unsupported')
    for role in [dict(kind='Projection',index_id='index.v1'),dict(kind='Government',benchmark_id='gov.v1'),dict(kind='Repo',collateral_id='bond.v1')]:
        c=copy.deepcopy(seed);c['role']=role
        add('monotone.role.'+role['kind'],request([c],dict(kind='curves')))
    equivalent=copy.deepcopy(seed);equivalent['key']=key('monotone.equivalent');equivalent['dependencies']=[seed['key']]
    equivalent['source']=dict(DiscountEquivalent=dict(base=seed['key']));equivalent['representation']='DiscountFactors'
    add('monotone.view.exact',request([equivalent,seed],dict(kind='curve_query',curve=equivalent['key'],query=dict(kind='Raw',point=dict(Tenor=12.0)))))
    for count in [1024,1025]:
        c=copy.deepcopy(seed);c['source']['MonotoneConvex']['nodes']=[dict(pillar=dict(Tenor=(i+1)/8),quote=dict(value=.03125,unit='DecimalRate')) for i in range(count)]
        add('monotone.nodes.'+str(count),request([c],dict(kind='curves')),'resource_limit' if count>1024 else None)
    return dict(schema_version=1,certified=False,qualification='Complete supplied nonnegative MonotoneConvex engineering corpus. Fresh native envelopes are transport expectations; frozen independent economics remain separate.',cases=cases)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--check',action='store_true');args=p.parse_args()
    data=generate();raw=(json.dumps(data,sort_keys=True,separators=(',',':'))+'\n').encode()
    if args.check:
        if OUTPUT.read_bytes()!=raw:raise SystemExit('Full original monotone input corpus differs')
    else:OUTPUT.write_bytes(raw)
    print(json.dumps(dict(status='passed',cases=len(data['cases']),successes=sum(c['expected_error'] is None for c in data['cases']))))
