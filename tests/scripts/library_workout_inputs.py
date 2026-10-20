#!/usr/bin/env python3
"""Full supplied workout requests; independent controls are a separate artifact."""
import argparse
import copy
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
OUTPUT=ROOT/'tests/fixtures/library/workouts.json'
INVERSE=dict(lower=-0.01,upper=0.25,price_tolerance=1e-8,parameter_tolerance=1e-12,max_iterations=96)
CONTROLS=dict(inverse=INVERSE,max_dates=64,max_cash_evaluations=1_000_000)


def generate():
    r=json.loads((ROOT/'tests/fixtures/library/workout-reference.json').read_bytes())
    cases=[]
    def request(c,action):
        context=copy.deepcopy(c['context']);context['trade_date']='2026-05-04'
        return dict(schema_version=1,inputs=dict(calendars=copy.deepcopy(r['calendars']),helper_calendars=[],curves=[],calibration=None,fixings=[],publications=[]),
                    operation=dict(kind='workout',input=dict(reference=dict(id='workout.original.reference',version=1),quote_revision=dict(id='workout.observed.quote',version=42),bond=copy.deepcopy(c['bond'])),
                                   context=context,convention=copy.deepcopy(c['convention']),action=copy.deepcopy(action)))
    def add(identity,q,error=None):
        cases.append(dict(id=identity,request=q,expected_error=error)); return q
    for c in r['cases']:
        suffix=c['id'].removeprefix('workout.reference.')
        add('workout.price.'+suffix,request(c,dict(kind='price',selection=c['selection'],decimal_yield=float(c['decimal_yield']))))
        add('workout.yield.'+suffix,request(c,dict(kind='yield',selection=c['selection'],target=dict(kind='dirty',value=float(c['dirty_per100'])),settings=INVERSE)))
    base=add('workout.worst.calls_and_maturity',request(r['all_dates'],dict(kind='worst',target=dict(kind='dirty',value=float(r['all_dates']['dirty_per100'])),settings=CONTROLS)))
    exact=copy.deepcopy(base);exact['operation']['action']['settings'].update(max_dates=14,max_cash_evaluations=10816)
    add('workout.worst.exact_budgets',exact)
    filtered=copy.deepcopy(base);filtered['operation']['context']['notice']['earliest_notice']='2027-06-17'
    add('workout.worst.notice_filter',filtered)
    for identity,provision,exercise,target in [('call',0,'2027-06-15',72.0),('put',1,'2027-07-15',71.0)]:
        q=copy.deepcopy(base);q['operation']['context'].update(reference_date='2027-06-02',trade_date='2027-06-01',settlement='2027-06-03',
            notice=dict(kind='notified',provision=provision,exercise=exercise,notice_date='2027-06-01'))
        q['operation']['action']['target']['value']=target
        add('workout.worst.notified_'+identity,q)
    maturity=next(c['request'] for c in cases if c['id']=='workout.price.Annual.0.04125.maturity.0')
    plain=copy.deepcopy(maturity);plain['operation']['input']['bond']['terms']['exercise']=[]
    add('workout.price.plain_maturity',plain)
    singleton=next(c['request'] for c in cases if c['id']=='workout.yield.SemiAnnual.0.04125.exercise.0')
    # Source revisions are separate and roundtrip; no guessed provenance is added.
    revision=copy.deepcopy(singleton);revision['operation']['input']['reference']['version']=7
    revision['operation']['input']['quote_revision']['version']=99
    add('workout.yield.independent_revisions',revision)
    def edit(q,path,value):
        parts=path.split('.');node=q
        for part in parts[:-1]:node=node[int(part)] if isinstance(node,list) else node[part]
        node[int(parts[-1]) if isinstance(node,list) else parts[-1]]=value
    specs=[
        ('missing_calendar',base,'inputs.calendars',[],'missing_input'),
        ('calendar_revision',base,'operation.input.bond.calendars.exercise.version',99,'missing_input'),
        ('calendar_name',base,'operation.input.bond.terms.calendars.exercise','wrong.exercise.name','invalid_input'),
        ('invalid_principal',base,'operation.input.bond.terms.face_value','0','invalid_input'),
        ('overlapping_provision',base,'operation.input.bond.terms.exercise.2.style.start','2027-06-14','invalid_input'),
        ('information_after_purchase',base,'operation.context.reference_date','2026-05-07','invalid_input'),
        ('information_before_issue',base,'operation.context.reference_date','2025-12-31','invalid_input'),
        ('purchase_lag',base,'operation.context.settlement','2026-05-07','invalid_input'),
        ('earliest_before_information',base,'operation.context.notice.earliest_notice','2026-05-03','invalid_input'),
        ('date_budget',base,'operation.action.settings.max_dates',13,'resource_limit'),
        ('cash_budget',exact,'operation.action.settings.max_cash_evaluations',10815,'resource_limit'),
        ('zero_dates',base,'operation.action.settings.max_dates',0,'invalid_input'),
        ('unbounded_dates',base,'operation.action.settings.max_dates',16385,'invalid_input'),
        ('zero_cash_budget',base,'operation.action.settings.max_cash_evaluations',0,'invalid_input'),
        ('unbounded_cash_budget',base,'operation.action.settings.max_cash_evaluations',67108865,'invalid_input'),
        ('zero_price_tolerance',base,'operation.action.settings.inverse.price_tolerance',0.0,'invalid_input'),
        ('parameter_width_only',base,'operation.action.settings.inverse.parameter_tolerance',1e-5,'invalid_input'),
        ('all_path_no_root',base,'operation.action.target.value',1000.0,'no_root'),
        ('mixed_sign_cash',base,'operation.input.bond.terms.profiles.coupon.0.value','-0.5','unsupported'),
        ('outside_exercise',singleton,'operation.action.selection.date','2027-06-14','invalid_input'),
        ('missing_provision',singleton,'operation.action.selection.provision',99,'invalid_input'),
        ('late_new_notice',singleton,'operation.context.notice.earliest_notice','2027-06-05','invalid_input'),
        ('past_exercise',singleton,'operation.context.settlement','2027-06-15','invalid_input'),
        ('maturity_exercise',singleton,'operation.action.selection.date','2029-01-01','invalid_input'),
        ('no_root',singleton,'operation.action.target.value',1000.0,'no_root'),
        ('one_iteration',singleton,'operation.action.settings.max_iterations',1,'no_convergence'),
        ('zero_iterations',singleton,'operation.action.settings.max_iterations',0,'invalid_input'),
        ('invalid_domain',singleton,'operation.action.settings.lower',-2.0,'no_convergence'),
        ('unsupported_contextual_yield',singleton,'operation.convention.day_count','ActActIcma','unsupported'),
    ]
    for name,original,path,v,error in specs:
        q=copy.deepcopy(original);edit(q,path,v)
        if name=='past_exercise':q['operation']['context'].update(trade_date='2027-06-14',settlement='2027-06-17')
        add('workout.failure.'+name,q,error)
    for family,identity,changes in [
        ('price','workout.price.SemiAnnual.0.04125.exercise.0',[
            ('invalid_yield','operation.action.decimal_yield',11.0),
            ('outside_date','operation.action.selection.date','2027-06-14'),
            ('notice_deadline','operation.context.notice.earliest_notice','2027-06-05')]),
        ('put','workout.yield.SemiAnnual.0.04125.exercise.1',[
            ('outside_date','operation.action.selection.date','2027-07-14'),
            ('notice_deadline','operation.context.notice.earliest_notice','2027-07-06'),
            ('no_root','operation.action.target.value',1000.0)])]:
        original=next(c['request'] for c in cases if c['id']==identity)
        for name,path,v in changes:
            q=copy.deepcopy(original);edit(q,path,v)
            add('workout.failure.'+family+'.'+name,q,'no_root' if name=='no_root' else 'invalid_input')
    notified=next(c['request'] for c in cases if c['id']=='workout.worst.notified_call')
    for name,path,value in [('notice_unknown','operation.context.notice.notice_date','2027-06-03'),
                            ('notice_late','operation.context.notice.notice_date','2027-06-05'),
                            ('notice_wrong_provision','operation.context.notice.provision',1),
                            ('notice_wrong_date','operation.context.notice.exercise','2027-06-16')]:
        q=copy.deepcopy(notified);edit(q,path,value)
        if name=='notice_late':q['operation']['context'].update(reference_date='2027-06-08',trade_date='2027-06-08',settlement='2027-06-10')
        add('workout.failure.'+name,q,'invalid_input')
    q=copy.deepcopy(singleton);q['operation']['context']=copy.deepcopy(notified['operation']['context']);q['operation']['action']['selection']=dict(kind='maturity')
    add('workout.failure.notified_maturity',q,'invalid_input')
    # A final unselected holder-put path must still pass the original inverse.
    q=copy.deepcopy(base);q['operation']['input']['bond']['terms']['exercise'][1]['price_per_100']='900'
    add('workout.failure.unselected_put_no_root',q,'no_root')
    original=json.loads((ROOT/'tests/fixtures/library/cases.json').read_bytes())
    frn_case=next(c['request'] for c in original['cases'] if c['id']=='frn.price')
    frn=copy.deepcopy(frn_case['operation']['bond'])
    q=copy.deepcopy(base);q['operation']['input']['bond']=frn
    for calendar in frn_case['inputs']['calendars']:
        if all(c['pin']!=calendar['pin'] for c in q['inputs']['calendars']):q['inputs']['calendars'].append(copy.deepcopy(calendar))
    q['operation']['context'].update(reference_date='2024-01-08',trade_date='2024-01-10',settlement='2024-01-10')
    q['operation']['context']['notice']['earliest_notice']='2024-01-09'
    add('workout.failure.floating',q,'unsupported')
    return dict(schema_version=1,certified=False,qualification='Supplied complete workout engineering requests. Independent predeclared cash/price/yield expectations remain in workout-reference.json; fresh native envelopes are transport expectations only.',cases=cases)


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--write',action='store_true');args=parser.parse_args()
    data=generate();content=(json.dumps(data,separators=(',',':'),sort_keys=True)+'\n').encode()
    if args.write: OUTPUT.write_bytes(content)
    elif OUTPUT.read_bytes()!=content: raise ValueError('Original supplied workout inputs changed')
    print(json.dumps(dict(status='passed',cases=len(data['cases']),successes=sum(c['expected_error'] is None for c in data['cases']),certified=False)))


if __name__=='__main__':main()
