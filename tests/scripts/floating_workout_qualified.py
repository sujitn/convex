#!/usr/bin/env python3
"""Retain original raw/admitted cases and separately declare complete controls.

Original no-root brackets/answers are unchanged. Paired revision aliases and an
unused historical fixing mutation are diagnosed honestly, with separate actual
stale/negative and admissible full-path cases. No bound is widened.
"""
import argparse
import copy
from decimal import localcontext
import json
from pathlib import Path
import floating_workout_reference as ref

FOLDER=Path(__file__).resolve().parents[1]/'fixtures/library'
CASES=FOLDER/'floating-workout-qualified.json'
REFERENCE=FOLDER/'floating-workout-additional-reference.json'
def wire(request):
    r=copy.deepcopy(request)
    for f in r['inputs']['fixings']:f['source_id']='FLOAT.ORIGINAL.ADMINISTRATOR'
    p=r['operation']['input']['position'];p['quantity']=p.pop('signed_quantity');p['quantity_unit']=p['quantity_unit'].lower()
    return r
def controls():
    raw=json.loads((FOLDER/'floating-workouts.json').read_bytes())
    admitted=json.loads((FOLDER/'floating-workout-admitted.json').read_bytes())
    original={c['id']:c for c in raw['cases']};cases=copy.deepcopy(admitted['cases']);references=[]
    reassessed={'overnight.paths':'no_root','shifted.paths':'no_root',
        'invalid.stale_projection':None,'invalid.stale_calendar':None,'invalid.negative_reject':None}
    for c in cases:
        if c['id'] in reassessed:
            c['expected_error']=reassessed[c['id']]
            if c['expected_error'] is None:
                r=original[c['id']]['request'];a=r['operation']['action']
                references.append(dict(id='floating.'+c['id'],expected=ref.price(r,a['selection'],ref.D(str(a['decimal_dm'])))))
        c['id']='floating.'+c['id']
    def add(id_,r,error=None):
        id_='floating.qualified.'+id_;cases.append(dict(id=id_,request=wire(r),expected_error=error))
        if error:return
        a=r['operation']['action']
        if a['kind']=='price':expected=ref.price(r,a['selection'],ref.D(str(a['decimal_dm'])))
        else:
            paths,examined=ref.paths(r);quotes=[]
            for p in paths:
                prefix=next((c['weight_end'] for c in a['shifted_prefixes'] if c['path']==p),None)
                s=dict(path=p,weight_end=prefix)
                quotes.append(dict(selection=s,expected=ref.solve(r,s,a['target'],a['settings']['inverse'])))
            expected=dict(quotes=quotes,examined_dates=examined)
        references.append(dict(id=id_,expected=expected))
    for name in ['overnight','shifted']:
        r=copy.deepcopy(original[name+'.paths']['request']);ref.terms(r)['redemption_multiplier']='1'
        add(name+'.paths',r)
    r=ref.base();fixing=r['operation']['input']['projection_context']['fixing']
    fixing.update(valuation_date='2024-01-02',known_at='2024-01-02T12:00:00Z')
    r['operation']['context'].update(reference_date='2024-01-02',trade_date='2024-01-02',settlement='2024-01-04',notice=dict(kind='unnotified',earliest_notice='2024-01-02'))
    r['operation']['input']['projection_context']['settlement']='2024-01-04'
    for c in r['inputs']['curves']:c['reference_date']='2024-01-02'
    add('awaiting_publication',r)
    r=copy.deepcopy(r);fixing=r['operation']['input']['projection_context']['fixing'];fixing['unpublished']='RequirePublished'
    add('awaiting_forbidden',r,'missing_input')
    r=ref.base();r['operation']['input']['projection_context']['fixing']['known_at']='2024-03-04T18:00:00Z'
    r['inputs']['fixings'].append(dict(index_id='USD.SUPPLIED.TERM',observation_date='2024-02-02',rate='0.5',published_at='2024-02-02T16:00:00Z',known_at='2024-02-02T16:01:00Z',revision=1,status='Final'))
    add('future_history_cannot_replace_forecast',r)
    r=ref.base();old=next(f for f in r['inputs']['fixings'] if f['observation_date']=='2024-01-02')
    late=copy.deepcopy(old);late.update(rate='0.06',revision=2,published_at='2024-01-04T16:00:00Z',known_at='2024-01-04T16:01:00Z');r['inputs']['fixings'].append(late)
    add('late_revision_excluded',r)
    r=ref.base();next(f for f in r['inputs']['fixings'] if f['observation_date']=='2024-01-02')['status']='Preliminary'
    add('preliminary_forbidden',r,'missing_input')
    r=copy.deepcopy(r);r['operation']['input']['projection_context']['fixing']['final_only']=False
    add('preliminary_explicit',r)
    r=ref.base();r['operation']['input']['projection']=dict(id='FLOAT.PROJECTION',version='stale-original')
    add('stale_projection',r,'missing_input')
    r=ref.base();r['operation']['input']['security']['bond']['calendars']['exercise']=dict(id='FLOAT.EXERCISE',version=99)
    add('stale_calendar',r,'missing_input')
    r=ref.base();ref.terms(r)['negative_coupons']='Reject'
    next(f for f in r['inputs']['fixings'] if f['observation_date']=='2024-01-02')['rate']='-0.5'
    add('negative_rejected',r,'invalid_input')
    r=ref.base(True,True,2);r['operation']['action']['selection']['weight_end']='2024-09-25'
    for name in ['coupon','spread']:
        for p in ref.terms(r)['profiles'][name]:p['value']='0'
    for f in r['inputs']['fixings']:f['rate']='0'
    for node in r['inputs']['curves'][1]['source']['Nodes']:node['quote']['value']=0.0
    add('zero_partial_invalid_cutoff',r,'invalid_input')
    r=ref.base();r['operation']['input']['projection_context']['on_payment']='Include'
    add('payment_boundary_mismatch',r,'invalid_input')
    r=copy.deepcopy(original['shifted.paths']['request']);r['operation']['context']['notice']=dict(kind='notified',provision=0,exercise='2024-09-29',notice_date='2024-09-27')
    r['operation']['action']['shifted_prefixes']=[r['operation']['action']['shifted_prefixes'][0]]
    add('forced_paths',r)
    for action in ['price','paths']:
        r=copy.deepcopy(original['notice.forced']['request'])
        if action=='price':r['operation']['action']['selection']=dict(path=dict(kind='maturity'),weight_end=None)
        else:r['operation']['action']=dict(kind='paths',target=dict(kind='dirty',value=100),settings=dict(inverse=ref.SETTINGS,max_dates=32,max_cash_evaluations=1_000_000),shifted_prefixes=[dict(path=dict(kind='exercise',provision=2,date='2024-10-03'),weight_end='2024-10-01')])
        add('forced_bad_'+action,r,'invalid_input')
    return dict(schema_version=1,certified=False,qualification='Full original cases retained with explicit diagnosis of paired revision aliases, unused fixing mutation and no-root original domains. Separate financial controls declared independently before execution; no bounds/answers changed.',cases=cases),dict(precision=60,certified=False,absolute_cash_price_tolerance='0.00000001',absolute_dm_tolerance='0.000000001',relative_tolerance='0',cases=references)
if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('action',choices=['generate','check']);args=parser.parse_args()
    with localcontext() as ctx:ctx.prec=60;cases,reference=controls()
    for path,value in [(CASES,cases),(REFERENCE,reference)]:
        raw=(json.dumps(value,indent=2)+'\n').encode()
        if args.action=='generate':
            if path.exists():raise ValueError('Refuse to overwrite independently declared qualification')
            path.write_bytes(raw)
        elif path.read_bytes()!=raw:raise ValueError('Original separate qualification changed: '+str(path))
    print(json.dumps(dict(status='passed',requests=len(cases['cases']),additional_independent_cases=len(reference['cases']),certified=False)))
