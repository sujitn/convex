#!/usr/bin/env python3
"""Complete supplied curve-family requests; independent financial controls are separate."""
import argparse
import copy
import datetime as dt
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
OUTPUT=ROOT/'tests/fixtures/library/curve-families.json'
REFERENCE=ROOT/'tests/fixtures/library/curve-family-reference.json'


def key(identity): return dict(id=identity,version='source.v7')


def recipe(identity,source,method='Linear',tail='None'):
    return dict(key=key(identity),role=dict(kind='Discount',collateral_currency='USD'),currency='USD',reference_date='2024-02-29',
                time_basis='Act365Fixed',representation=dict(ZeroRates=dict(compounding='Continuous')),interpolation=method,
                extrapolation=tail,dependencies=[],calibration='Direct',source=source)


def request(curves,operation):
    return dict(schema_version=1,inputs=dict(calendars=[],helper_calendars=[],curves=curves,calibration=None,fixings=[],publications=[]),operation=operation)


def generate():
    ref=json.loads(REFERENCE.read_bytes());cases=[]
    def add(identity,q,error=None): cases.append(dict(id=identity,request=copy.deepcopy(q),expected_error=error));return q
    def query(identity,curves,target,kind,t):
        return add(identity,request(curves,dict(kind='curve_query',curve=key(target),query=dict(kind=kind,point=dict(Tenor=float(t))))))
    models={m['id']:recipe(m['id'],dict(Parametric=dict(parameters={k:v for k,v in m.items() if k!='id'},horizon=dict(Tenor=150.0),observations=[])),
                'NelsonSiegel' if m['kind']=='nelson_siegel' else 'Svensson') for m in ref['models']}
    for c in ref['cases']:
        identity=next(m['id'] for m in ref['models'] if c['id'].startswith(m['id']+'.'))
        for kind,suffix in [('Raw','zero'),('DiscountFactor','discount'),('InstantaneousForward','forward')]:
            query('curve.model.'+suffix+'.'+c['id'],[models[identity]],identity,kind,c['tenor'])
    for identity,model in models.items():
        checked=copy.deepcopy(model);checked['source']['Parametric']['observations']=ref['observations'][identity]
        add('curve.model.observations.'+identity,request([checked],dict(kind='curves')))
        fit=copy.deepcopy(checked);p=fit['source']['Parametric']['parameters'];p['beta0']+=0.0005;p['beta1']+=0.001;p['beta2']-=0.0005
        settings=copy.deepcopy(ref['fit_settings'])
        if p['kind']=='svensson':
            settings['lower']=[-0.1,-0.1,-0.1,-0.1,0.1,0.1];settings['upper']=[0.1,0.1,0.1,0.1,10.0,30.0]
        add('curve.model.fit.'+identity,request([],dict(kind='parametric_fit',recipe=fit,settings=settings)))
        add('curve.model.fit.exact_start.'+identity,request([],dict(kind='parametric_fit',recipe=checked,settings=settings)))
        add('curve.model.describe.'+identity,request([model],dict(kind='curves')))
    short=recipe('short',dict(Nodes=[dict(pillar=dict(Tenor=t),quote=dict(value=v,unit='DecimalRate')) for t,v in [(0.0,0.03),(2.0,0.04)]]))
    long=recipe('long',dict(Nodes=[dict(pillar=dict(Tenor=t),quote=dict(value=v,unit='DecimalRate')) for t,v in [(2.0,0.04),(4.0,0.035),(5.0,0.0325)]]))
    full=recipe('full',dict(Nodes=[dict(pillar=dict(Tenor=t),quote=dict(value=v,unit='DecimalRate')) for t,v in [(0.0,0.03),(5.0,0.055)]]))
    segmented=recipe('segments',dict(Segmented=dict(segments=[dict(start=dict(Tenor=0.0),end=dict(Tenor=2.0),base=key('short'),fallback='Strict'),
                      dict(start=dict(Tenor=2.0),end=dict(Tenor=5.0),base=key('long'),fallback='Strict')],join_tolerance=1e-12)),'Segmented')
    segmented['dependencies']=[key('short'),key('long')]
    for c in ref['segments']:
        for kind in ['Raw','DiscountFactor']:query('curve.segment.'+kind+'.'+c['tenor'],[segmented,long,short],'segments',kind,c['tenor'])
    add('curve.segment.describe',request([segmented,long,short],dict(kind='curves')))
    for fallback in ['Strict','Clamp','FlatExtrapolation','Trust']:
        range_=dict(start=dict(Tenor=0.0 if fallback=='Trust' else 1.0),end=dict(Tenor=5.0 if fallback=='Trust' else 4.0))
        delegated=recipe('delegate.'+fallback,dict(Delegated=dict(base=key('full'),range=range_,fallback=fallback)),'Delegated',
                         'Flat' if fallback in ['Clamp','FlatExtrapolation'] else 'None');delegated['dependencies']=[key('full')]
        for c in ref['delegated']:
            t=float(c['tenor']);q=request([delegated,full],dict(kind='curve_query',curve=key('delegate.'+fallback),query=dict(kind='Raw',point=dict(Tenor=t))))
            error='invalid_input' if (fallback=='Strict' and not 1.0<=t<=4.0) or (fallback=='Trust' and t>5.0) else None
            add('curve.delegate.'+fallback+'.'+c['tenor'],q,error)
    # Exact existing derived/rolled views retain the new native source; no resampling.
    spread=copy.deepcopy(models['ns.positive']);spread['key']=key('model.spread');spread['dependencies']=[key('ns.positive')]
    spread['source']=dict(ContinuousZeroSpread=dict(base=key('ns.positive'),spread=dict(value=12.5,unit='BasisPoints')))
    query('curve.model.spread',[spread,models['ns.positive']],'model.spread','DiscountFactor',2.0)
    for policy in ['StickyForward','StickyZero']:
        rolled=copy.deepcopy(segmented);rolled['key']=key('roll.'+policy);rolled['reference_date']='2025-02-28';rolled['representation']='DiscountFactors'
        rolled['dependencies']=[key('segments')];rolled['source']=dict(Rolled=dict(base=key('segments'),policy=policy))
        query('curve.segment.roll.'+policy,[rolled,segmented,long,short],'roll.'+policy,'DiscountFactor',1.0)
    clamp_segment=recipe('inner.tail',dict(Segmented=dict(segments=[dict(start=dict(Tenor=0.0),end=dict(Tenor=10.0),base=key('full'),fallback='Clamp')],join_tolerance=1e-12)),'Segmented')
    clamp_segment['dependencies']=[key('full')]
    query('curve.segment.inner_tail',[clamp_segment,full],'inner.tail','Raw',9.0)
    flat=copy.deepcopy(models['ns.positive']);flat['extrapolation']='Flat';flat['source']['Parametric']['horizon']=dict(Tenor=30.0)
    query('curve.model.flat_tail',[flat],'ns.positive','Raw',150.0)
    for dc,days in [('Act360',360),('Act365Fixed',365)]:
        dated=copy.deepcopy(models['ns.negative']);dated['time_basis']=dc
        date=str(dt.date(2024,2,29)+dt.timedelta(days=days))
        add('curve.model.dated.'+dc,request([dated],dict(kind='curve_query',curve=key('ns.negative'),query=dict(kind='Raw',point=dict(Date=date)))))
    add('curve.segment.roundtrip',request([segmented,long,short],dict(kind='roundtrip')))
    # Supplied native boundaries retain every source without an unbounded tail.
    many=copy.deepcopy(segmented);many['source']['Segmented']['segments']=[dict(start=dict(Tenor=i/32),end=dict(Tenor=(i+1)/32),base=key('full'),fallback='Strict') for i in range(128)]
    many['dependencies']=[key('full')]
    add('curve.segment.max128',request([many,full],dict(kind='curves')))
    too_many=copy.deepcopy(many);too_many['source']['Segmented']['segments'].append(dict(start=dict(Tenor=4.0),end=dict(Tenor=4.03125),base=key('full'),fallback='Strict'))
    add('curve.failure.segment129',request([too_many,full],dict(kind='curves')),'resource_limit')
    chain=[full];base='full'
    for i in range(33):
        name='depth.'+str(i+1);link=recipe(name,dict(Delegated=dict(base=key(base),range=dict(start=dict(Tenor=0.0),end=dict(Tenor=5.0)),fallback='Trust')),'Delegated')
        link['dependencies']=[key(base)];chain.append(link);base=name
        if i==31:query('curve.delegate.max32',chain,base,'Raw',1.0)
    add('curve.failure.depth33',request(chain,dict(kind='curves')),'resource_limit')
    def edit(q,path,value):
        n=q;parts=path.split('.')
        for p in parts[:-1]: n=n[int(p)] if isinstance(n,list) else n[p]
        n[int(parts[-1]) if isinstance(n,list) else parts[-1]]=value
    model_q=request([models['ns.positive']],dict(kind='curves'))
    segment_q=request([segmented,long,short],dict(kind='curves'))
    specs=[
        ('model_tau',model_q,'inputs.curves.0.source.Parametric.parameters.tau',0.0,'invalid_input'),
        ('model_beta',model_q,'inputs.curves.0.source.Parametric.parameters.beta0',10.1,'no_convergence'),
        ('model_method',model_q,'inputs.curves.0.interpolation','Svensson','invalid_input'),
        ('model_representation',model_q,'inputs.curves.0.representation','DiscountFactors','invalid_input'),
        ('model_horizon',model_q,'inputs.curves.0.source.Parametric.horizon.Tenor',0.0,'invalid_input'),
        ('model_tail',model_q,'inputs.curves.0.extrapolation','FlatForward','unsupported'),
        ('missing_source',segment_q,'inputs.curves.0.source.Segmented.segments.1.base.version','missing.v8','invalid_input'),
        ('missing_dependency',segment_q,'inputs.curves.0.dependencies.1.version','missing.v8','invalid_input'),
        ('different_currency',segment_q,'inputs.curves.1.currency','EUR','invalid_input'),
        ('different_basis',segment_q,'inputs.curves.1.time_basis','Act360','invalid_input'),
        ('different_reference',segment_q,'inputs.curves.1.reference_date','2024-03-01','invalid_input'),
        ('different_role',segment_q,'inputs.curves.1.role',dict(kind='Government',benchmark_id='actual.government'),'invalid_input'),
        ('gap',segment_q,'inputs.curves.0.source.Segmented.segments.1.start.Tenor',2.1,'invalid_input'),
        ('overlap',segment_q,'inputs.curves.0.source.Segmented.segments.1.start.Tenor',1.9,'invalid_input'),
        ('horizon',segment_q,'inputs.curves.0.source.Segmented.segments.1.end.Tenor',6.0,'invalid_input'),
        ('zero_join_bound',segment_q,'inputs.curves.0.source.Segmented.join_tolerance',0.0,'invalid_input'),
        ('bad_join',segment_q,'inputs.curves.1.source.Nodes.0.quote.value',0.041,'no_convergence'),
        ('wrong_method',segment_q,'inputs.curves.0.interpolation','Linear','invalid_input'),
    ]
    for name,q,path,value,error in specs:
        q=copy.deepcopy(q);edit(q,path,value);add('curve.failure.'+name,q,error)
    observed=next(c['request'] for c in cases if c['id']=='curve.model.observations.ns.positive')
    for name,path,value,error in [
        ('wrong_quote','inputs.curves.0.source.Parametric.observations.0.quote.value',0.1,'no_convergence'),
        ('quote_unit','inputs.curves.0.source.Parametric.observations.0.quote.unit','PricePer100','invalid_input'),
        ('quote_revision','inputs.curves.0.source.Parametric.observations.0.version','','invalid_input'),
        ('quote_date','inputs.curves.0.source.Parametric.observations.0.pillar','2024-02-29','invalid_input'),
        ('duplicate_quote','inputs.curves.0.source.Parametric.observations.1.id','zero.0','invalid_input')]:
        q=copy.deepcopy(observed);edit(q,path,value);add('curve.failure.'+name,q,error)
    fit=next(c['request'] for c in cases if c['id']=='curve.model.fit.ns.positive')
    for name,path,value,error in [
        ('fit_zero_iterations','operation.settings.max_iterations',0,'invalid_input'),
        ('fit_one_iteration','operation.settings.max_iterations',1,'no_convergence'),
        ('fit_scale','operation.settings.quote_scales.0',0.0,'invalid_input'),
        ('fit_weights','operation.settings.weights',[0.0]*10,'invalid_input'),
        ('fit_bounds','operation.settings.lower.3',0.0,'invalid_input'),
        ('fit_wrong_dimension','operation.settings.lower',[0.0],'invalid_input')]:
        q=copy.deepcopy(fit);edit(q,path,value)
        if name=='fit_one_iteration': q['operation']['recipe']['source']['Parametric']['parameters']['tau']=1.0
        add('curve.failure.'+name,q,error)
    q=copy.deepcopy(fit);p=q['operation']['recipe']['source']['Parametric'];p['parameters'].update(beta0=0.04,beta1=0.0,beta2=0.0)
    for o in p['observations']:o['quote']['value']=4.0 if o['quote']['unit']=='PercentRate' else 0.04
    add('curve.failure.fit_unidentified',q,'non_unique')
    q=copy.deepcopy(segment_q);q['inputs']['curves']=q['inputs']['curves'][:1]
    add('curve.failure.absent_dependency',q,'missing_input')
    q=copy.deepcopy(segment_q);q['inputs']['curves'][0]['dependencies'].append(key('short'))
    add('curve.failure.duplicate_dependency',q,'invalid_input')
    q=copy.deepcopy(segment_q);q['inputs']['curves'][0]['source']['Segmented']['segments'].reverse()
    add('curve.failure.unordered_segments',q,'invalid_input')
    query_q=request([models['ns.positive']],dict(kind='curve_query',curve=key('ns.positive'),query=dict(kind='Raw',point=dict(Tenor=151.0))))
    add('curve.failure.query_ceiling',query_q,'invalid_input')
    add('curve.failure.forward_tail',request([flat],dict(kind='curve_query',curve=key('ns.positive'),query=dict(kind='InstantaneousForward',point=dict(Tenor=31.0)))),'unsupported')
    add('curve.failure.grid_forward',request([full],dict(kind='curve_query',curve=key('full'),query=dict(kind='InstantaneousForward',point=dict(Tenor=1.0)))),'unsupported')
    q=request([models['sv.positive']],dict(kind='curves'));q['inputs']['curves'][0]['source']['Parametric']['parameters']['tau2']=0.0
    add('curve.failure.svensson_tau',q,'invalid_input')
    q=copy.deepcopy(next(c['request'] for c in cases if c['id']=='curve.model.fit.sv.positive'));q['operation']['settings']['max_iterations']=1
    q['operation']['recipe']['source']['Parametric']['parameters'].update(tau1=1.0,tau2=4.0)
    add('curve.failure.svensson_fit_iterations',q,'no_convergence')
    q=copy.deepcopy(fit);q['operation']['settings']['weights'][-1]=0.0;q['operation']['recipe']['source']['Parametric']['observations'][-1]['quote']['value']+=0.01
    add('curve.failure.zero_weight_native_residual',q,'no_convergence')
    for fallback in ['Strict','Clamp','FlatExtrapolation','Trust']:
        q=copy.deepcopy(next(c['request'] for c in cases if c['id']=='curve.delegate.'+fallback+'.3'))
        q['inputs']['curves'][0]['source']['Delegated']['range']['end']['Tenor']=0.5
        add('curve.failure.delegate_range.'+fallback,q,'invalid_input')
    q=copy.deepcopy(model_q);q['operation']=dict(kind='curve_query',curve=key('ns.positive'),query=dict(kind='InstantaneousForward',point=dict(Tenor=0.0)))
    q['inputs']['curves'][0]['source']['Parametric']['parameters']['tau']=200.0
    add('curve.model.original_decay_domain',q)
    return dict(schema_version=1,certified=False,qualification='Complete supplied model/segment/delegation engineering requests; independent frozen analytic controls are separate. Native envelopes are transport expectations only.',cases=cases)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate();raw=(json.dumps(data,separators=(',',':'),sort_keys=True)+'\n').encode()
    if args.write: OUTPUT.write_bytes(raw)
    elif OUTPUT.read_bytes()!=raw: raise ValueError('Supplied original curve-family requests changed')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),successes=sum(c['expected_error'] is None for c in data['cases']),certified=False)))


if __name__=='__main__': main()
