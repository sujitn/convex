#!/usr/bin/env python3
"""Explicit method contracts; independent financial expected values are separate."""
import argparse
import copy
import datetime as dt
import json
from pathlib import Path

from library_curve_family_inputs import key, recipe, request

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT/'tests/fixtures/library/curve-methods.json'


def generate():
    controls = json.loads((ROOT/'tests/fixtures/library/curve-method-reference.json').read_bytes())
    cases = []
    def add(id, q, error=None):
        cases.append(dict(id=id,request=copy.deepcopy(q),expected_error=error))
        return q
    def query(id, r, kind, t, side=None, error=None):
        action = dict(kind=kind,point=dict(Tenor=float(t)))
        if side: action['side'] = side
        return add(id,request([r],dict(kind='curve_query',curve=r['key'],query=action)),error)
    models = {}
    for grid in controls['grids']:
        method = grid['method']
        nodes = [dict(pillar=dict(Tenor=float(t)),
                      quote=dict(value=float(z)*(100 if i%2 else 1),unit='PercentRate' if i%2 else 'DecimalRate'))
                 for i,(t,z) in enumerate(zip(grid['knots'],grid['zeros']))]
        r = recipe(grid['id'],{method:dict(nodes=nodes)},method)
        models[grid['id']] = r
        add('method.describe.'+grid['id'],request([r],dict(kind='curves')))
        add('method.roundtrip.'+grid['id'],request([r],dict(kind='roundtrip')))
        for p in grid['points']:
            for kind in ['Raw','DiscountFactor']:
                query('method.'+kind+'.'+grid['id']+'.'+p['time'],r,kind,p['time'])
            if method=='FlatForward':
                for side in ['Left','Right']:
                    error = None if p[side.lower()] is not None else ('invalid_input' if side=='Left' else 'unsupported')
                    query('method.forward.'+side+'.'+grid['id']+'.'+p['time'],r,'FlatForwardRate',p['time'],side,error)
        for p in grid['tails']:
            tail = copy.deepcopy(r);tail['extrapolation'] = p['extension']
            for kind in ['Raw','DiscountFactor']:
                query('method.tail.'+p['extension']+'.'+kind+'.'+grid['id']+'.'+p['time'],tail,kind,p['time'])
            if p['extension']=='FlatForward':
                for side in ['Left','Right']:
                    query('method.tail.forward.'+side+'.'+grid['id']+'.'+p['time'],tail,'FlatForwardRate',p['time'],side)
        period = grid['interval']
        add('method.interval.'+grid['id'],request([r],dict(kind='curve_query',curve=r['key'],query=dict(
            kind='ForwardRate',start=dict(Tenor=float(period['start'])),end=dict(Tenor=float(period['end'])),
            compounding='Continuous',day_count='Act365Fixed'))))
    ff = models['FlatForward.positive']
    pc = models['PiecewiseConstant.positive']
    for r in [ff,pc]:
        method = r['interpolation']
        for dc,days in [('Act365Fixed',365),('Act360',360)]:
            dated = copy.deepcopy(r);dated['time_basis']=dc
            nodes = dated['source'][method]['nodes']
            for node in nodes:
                node['pillar']=dict(Date=str(dt.date(2024,2,29)+dt.timedelta(days=int(node['pillar']['Tenor']*days))))
            q = request([dated],dict(kind='curve_query',curve=r['key'],query=dict(
                kind='Raw',point=dict(Date=str(dt.date(2024,2,29)+dt.timedelta(days=int(1.5*days)))))))
            add('method.date.'+method+'.'+dc,q)
        shuffled=copy.deepcopy(r);shuffled['source'][method]['nodes'].reverse()
        add('method.shuffled.'+method,request([shuffled],dict(kind='curves')))
        full=copy.deepcopy(r);full['source'][method]['nodes']=[dict(pillar=dict(Tenor=i/16),quote=dict(value=0.03,unit='DecimalRate')) for i in range(1024)]
        add('method.limit1024.'+method,request([full],dict(kind='curves')))
        full['source'][method]['nodes'].append(dict(pillar=dict(Tenor=64.0),quote=dict(value=0.03,unit='DecimalRate')))
        add('method.failure.limit1025.'+method,request([full],dict(kind='curves')),'resource_limit')
        for role in [dict(kind='Government',benchmark_id='supplied.govt.v1'),
                     dict(kind='Projection',index_id='supplied.index.v1'),dict(kind='Repo',collateral_id='supplied.bond.v1')]:
            with_role=copy.deepcopy(r);with_role['role']=role
            add('method.role.'+method+'.'+role['kind'],request([with_role],dict(kind='curves')))
        for value in [0.5,0.0]:
            missing=copy.deepcopy(r);missing['source'][method]['nodes'].pop(0)
            query('method.failure.no_origin.'+method+'.'+str(value),missing,'Raw',value,error='invalid_input')
        query('method.failure.ceiling.'+method,r,'Raw',151,error='invalid_input')
        query('method.failure.before_date.'+method,r,'Raw',-1,error='invalid_input')
        query('method.failure.none_tail.'+method,r,'Raw',7,error='invalid_input')
    for method in ['PiecewiseConstant','FlatForward']:
        base = request([models[method+'.positive']],dict(kind='curves'))
        specs = [
            ('method','interpolation','Linear','invalid_input'),
            ('df','representation','DiscountFactors','unsupported'),
            ('periodic','representation',dict(ZeroRates=dict(compounding='SemiAnnual')),'unsupported'),
            ('issuer','role',dict(kind='IssuerSpread',entity_id='issuer.v1',model_id='basis.v1'),'invalid_input'),
            ('dependency','dependencies',[key('absent')],'invalid_input'),
            ('currency','currency','EUR','invalid_input'),
            ('calibration','calibration','Piecewise','unsupported'),
            ('ufr','extrapolation',dict(UfrConvergence=dict(ufr=0.03,alpha=0.1)),'unsupported'),
            ('version','key',dict(id=method+'.positive',version=''),'invalid_input'),
            ('basis','time_basis','ActActIcma','unsupported'),
        ]
        for name,field,value,error in specs:
            q=copy.deepcopy(base);q['inputs']['curves'][0][field]=value
            add('method.failure.'+name+'.'+method,q,error)
        for name,nodes,error in [
            ('empty',[],'invalid_input'),
            ('one',[dict(pillar=dict(Tenor=0.0),quote=dict(value=0.03,unit='DecimalRate'))],'invalid_input'),
            ('duplicate',base['inputs']['curves'][0]['source'][method]['nodes'][:1]*2,'invalid_input'),
            ('close',[dict(pillar=dict(Tenor=t),quote=dict(value=0.03,unit='DecimalRate')) for t in [0.0,1e-11]],'invalid_input'),
            ('wrong_unit',[dict(pillar=dict(Tenor=t),quote=dict(value=0.03,unit='PricePer100')) for t in [0.0,1.0]],'invalid_input'),
            ('horizon',[dict(pillar=dict(Tenor=t),quote=dict(value=0.03,unit='DecimalRate')) for t in [0.0,151.0]],'invalid_input'),
        ]:
            q=copy.deepcopy(base);q['inputs']['curves'][0]['source'][method]['nodes']=nodes
            add('method.failure.'+name+'.'+method,q,error)
    for side in ['Left','Right']:
        query('method.failure.step_forward.'+side,pc,'FlatForwardRate',1.5,side,'unsupported')
        for tail in ['Flat','Linear']:
            r=copy.deepcopy(ff);r['extrapolation']=tail
            query('method.failure.raw_tail_forward.'+tail+'.'+side,r,'FlatForwardRate',7,side,'unsupported')
        query('method.exact_tail_boundary.'+side,dict(ff,extrapolation='FlatForward'),'FlatForwardRate',5,side)
    r=copy.deepcopy(ff);r['source']['FlatForward']['nodes'][0]['quote']['value']=0.021
    add('method.failure.forward_origin',request([r],dict(kind='curves')),'invalid_input')
    r=copy.deepcopy(pc);r['extrapolation']='FlatForward'
    add('method.failure.step_tail',request([r],dict(kind='curves')),'unsupported')
    for method in ['PiecewiseConstant','FlatForward']:
        r=copy.deepcopy(models[method+'.positive']);r['source']=dict(Nodes=r['source'][method]['nodes'])
        add('method.failure.bare_nodes.'+method,request([r],dict(kind='curves')),'unsupported')
    r=copy.deepcopy(ff);r['source']['FlatForward']['nodes'].pop(0);r['extrapolation']='FlatForward'
    for t in [0,0.5,1,7]:
        for kind in ['Raw','DiscountFactor']:
            query('method.no_origin.tail.'+kind+'.'+str(t),r,kind,t)
        for side in ['Left','Right']:
            query('method.no_origin.forward.'+side+'.'+str(t),r,'FlatForwardRate',t,side,
                  'invalid_input' if t==0 and side=='Left' else None)
    return dict(schema_version=1,certified=False,
        qualification='Complete explicit method engineering requests; frozen analytic values are separate. Transport envelopes do not certify financial coverage.',cases=cases)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--check',action='store_true');args=p.parse_args()
    content=(json.dumps(generate(),sort_keys=True,separators=(',',':'))+'\n').encode()
    if args.check:
        if OUTPUT.read_bytes()!=content:raise SystemExit('Explicit method corpus differs')
    else:OUTPUT.write_bytes(content)
    doc=json.loads(content)
    print(json.dumps(dict(status='passed',cases=len(doc['cases']),successes=sum(c['expected_error'] is None for c in doc['cases']))))


if __name__=='__main__':main()
