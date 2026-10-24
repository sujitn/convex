"""Separate pre-execution admission controls; preserve every original raw answer."""
import argparse
from copy import deepcopy
from decimal import Decimal as D
import json
from pathlib import Path
from icma_government_reference import security, INVERSE
from icma_workout_reference import PATHS, make, legal_paths, value

ROOT = Path(__file__).resolve().parents[2]


def boundaries(request):
    t=request['operation']['input']['security']['bond']['terms'];s=t['schedule']
    return security('independent.boundary',s['end_date'],start=s['start_date'],frequency=s['frequency'],stub=s['stub_type'])[1]


def qualified():
    original=json.loads((ROOT/'tests/fixtures/library/icma-workout.json').read_bytes())
    cases=deepcopy(original['cases']);additional=[];declarations=[]
    for case in cases:
        op=case['request']['operation']; action=op['action']
        if case['expected_error'] is None and action['kind']=='worst':
            b=boundaries(case['request']);failed=[]
            for selection in legal_paths(case['request'])[0]:
                probe=deepcopy(case['request']);probe['operation']['action']=dict(kind='price_paths',decimal_yield=action['settings']['inverse']['lower'],settings=PATHS)
                low=value(probe,b,selection);probe['operation']['action']['decimal_yield']=action['settings']['inverse']['upper']
                high=value(probe,b,selection)
                target=D(str(action['target']['value']))+(D(low['accrued_per100']) if action['target']['kind']=='clean' else 0)
                a=D(low['dirty_per100'])-target;c=D(high['dirty_per100'])-target
                tolerance=D(str(action['settings']['inverse']['price_tolerance']))
                if a*c>0 and min(abs(a),abs(c))>tolerance:
                    failed.append(dict(selection=selection,low_residual=str(a),high_residual=str(c)))
            if failed:
                case['expected_error']='no_root'
                declarations.append(dict(id=case['id'],reason='One or more complete original legal paths exclude the observed target in the ORIGINAL closed domain; no path may be discarded.',failed_paths=failed))
        if case['id']=='icma_workout.invalid.ex_coupon_partial.price_paths':
            # Original payment-delay/ex-date lies after its accrual boundary,
            # and the proposed early date is also excluded by actual notice.
            # It therefore does not consume an ambiguous partial ex coupon.
            case['expected_error']=None
            b=boundaries(case['request']);selections,examined,forced=legal_paths(case['request'])
            additional.append(dict(id=case['id'],expected=dict(quotes=[value(case['request'],b,s) for s in selections],examined_dates=examined,forced_by_notice=forced,selected_index=None)))
            declarations.append(dict(id=case['id'],reason='Original post-accrual ex-date and notice exclusion do not create partial ownership ambiguity. Full independent cash/time/price/invoice answer declared separately.'))
        if case['id']=='icma_workout.invalid.ex_coupon_partial_before_inverse.worst':
            case['expected_error']='no_root'
            declarations.append(dict(id=case['id'],reason='Same admissible original context; original target10000 lies outside every original closed quotation bracket.'))
    def add(id,request,b,error=None):
        id='icma_workout.qualified.'+id;request=deepcopy(request)
        cases.append(dict(id=id,request=request,expected_error=error))
        if error is None:
            selections,examined,forced=legal_paths(request);quotes=[value(request,b,s) for s in selections]
            worst=request['operation']['action']['kind']=='worst'
            eligible=[i for i,q in enumerate(quotes) if forced or q['exercise'] is None or q['exercise']['right']=='call']
            index=min(eligible,key=lambda i:(D(quotes[i]['decimal_yield']),i)) if worst else None
            additional.append(dict(id=id,expected=dict(quotes=quotes,examined_dates=examined,forced_by_notice=forced,selected_index=index)))
    for comp in ['SemiAnnual','Continuous','Simple','Monthly']:
        r,b=make('negative_admitted');op=r['operation'];op['convention']['compounding']=comp
        op['context']['notice']['earliest_notice']='2024-03-10'
        op['action']=dict(kind='price_paths',decimal_yield=-0.0075,settings=PATHS)
        add('negative_yield.'+comp+'.price_paths',r,b)
        price=value(r,b,dict(kind='maturity'))['clean_per100']
        op['action']=dict(kind='worst',target=dict(kind='clean',value=float(price)),settings=dict(inverse=INVERSE,paths=PATHS))
        add('negative_yield.'+comp+'.worst',r,b)
    for label,quantity in [('positive_position','1234.56789'),('zero_position','0')]:
        r,b=make(label);op=r['operation'];op['input']['position']['quantity']=quantity
        add(label+'.price_paths',r,b)
        price=value(r,b,dict(kind='maturity'))['dirty_per100']
        op['action']=dict(kind='worst',target=dict(kind='dirty',value=float(price)),settings=dict(inverse=INVERSE,paths=PATHS))
        add(label+'.worst',r,b)
    r,b=make('true_partial_ex',dict(settle='2024-02-23',ex=True));op=r['operation'];t=op['input']['security']['bond']['terms']
    t['dates']['ex_coupon']['offset']=dict(CalendarDays=-20);t['exercise'][0]['style']['date']='2024-02-27'
    add('ex_coupon_partial.price_paths',r,b,'unsupported')
    op['action']=dict(kind='worst',target=dict(kind='dirty',value=10000.0),settings=dict(inverse=INVERSE,paths=PATHS))
    add('ex_coupon_partial_before_inverse.worst',r,b,'unsupported')
    r,b=make('failed_put');op=r['operation'];op['input']['security']['bond']['terms']['exercise'][1]['price_per_100']='1'
    add('unselected_put.price_paths',r,b)
    op['action']=dict(kind='worst',target=dict(kind='dirty',value=103.125),settings=dict(inverse=INVERSE,paths=PATHS))
    add('unselected_put_inverse.worst',r,b,'no_root')
    r,b=make('late_actual_notice',dict(settle='2024-03-02'));op=r['operation']
    op['context']['notice']=dict(kind='notified',provision=0,exercise='2024-03-05',notice_date='2024-03-02')
    add('late_forced_notice.price_paths',r,b,'invalid_input')
    r,b=make('protected_notice');op=r['operation']
    op['context']['notice']=dict(kind='notified',provision=2,exercise='2024-06-21',notice_date='2024-01-10')
    add('protected_forced_notice.price_paths',r,b,'invalid_input')
    return (dict(schema_version=1,certified=False,qualification='Every original122 supplied request and raw financial answer/bound remains. Independent original-endpoint admission declares genuinely rootless full-path requests. Original post-accrual ex-date/notice-excluded diagnostic is separately priced. Additional unchanged-domain negative-yield/signed quantity and actual partial-ex/failed-put/late/protected-notice controls precede the next candidate execution.',cases=cases),
            dict(certified=False,precision=60,cases=additional),
            dict(certified=False,original_requests=122,original_answers_unchanged=True,original_domains_and_bounds_unchanged=True,admission_declarations=declarations))


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();values=qualified()
    for path,value in zip(['icma-workout-qualified.json','icma-workout-additional-reference.json','icma-workout-admission.json'],values):
        file=ROOT/'tests/fixtures/library'/path
        if a.write:
            if file.exists():raise ValueError('Refusing to overwrite separate declarations: '+path)
            file.write_text(json.dumps(value,indent=2)+'\n')
        elif json.loads(file.read_bytes())!=value:raise ValueError('Original independent declaration changed: '+path)
    print(json.dumps(dict(requests=len(values[0]['cases']),successes=sum(c['expected_error'] is None for c in values[0]['cases']),failures=sum(c['expected_error'] is not None for c in values[0]['cases']),additional_complete_paths=sum(len(c['expected']['quotes']) for c in values[1]['cases']),admission_declarations=len(values[2]['admission_declarations']),certified=False)))
