"""Separate predeclared full multi-helper, payment and ex-coupon controls."""
import argparse
from copy import deepcopy as clone
from decimal import Decimal as D
import json
from pathlib import Path
from icma_helpers_reference import quote_price, curve_price, bisect
from icma_reference import day, fraction

ROOT=Path(__file__).resolve().parents[2]
def controls():
    cases=json.loads((ROOT/'tests/fixtures/library/icma-helpers.json').read_bytes())['cases']
    refs={r['id']:r['expected'] for r in json.loads((ROOT/'tests/fixtures/library/icma-helpers-reference.json').read_bytes())['cases']}
    rows=[]
    original=next(c for c in cases if c['id']=='icma_helpers.regular.SemiAnnual.0.04125.convert')
    base=original['request'];base_h=base['operation']['input']['helper'];e=refs[original['id']]['observed']
    for name,settle,payment,ex in [('payment_include','2024-03-12','Include',False),('payment_exclude','2024-03-12','Exclude',False),('pre_ex','2024-03-08','Exclude',True),('on_ex','2024-03-09','Exclude',True)]:
        r=clone(base);h=r['operation']['input']['helper'];h.update(trade_date=settle,settlement=settle,on_payment=payment);r['operation']['input']['reference_date']=settle
        if ex:h['terms']['dates']['ex_coupon']={'CalendarDays':-3}
        flow=clone(e['cashflows']);owned=(settle<'2024-03-12' or (settle=='2024-03-12' and payment=='Include')) and (not ex or settle<'2024-03-09')
        if not owned:flow=[c for c in flow if not (c['date']=='2024-03-12' and c['kind']=='coupon')]
        mesh=h['quote_kind']['input']['reference_context']['periods']
        accrued=D('0.048')*100*fraction(day('2024-02-29'),day(settle),mesh,2)+(D('2.4') if owned else 0)
        expected=quote_price(h,flow,accrued,D('.04125'))
        rows.append(dict(id='icma_helpers.boundary.'+name,request=r,expected_error=None,expected=dict(observed=expected)))
    # Two independent true last-cash pillars, all original coupons/principal;
    # nonflat zeros are converted to native contextual yield observations once.
    short=next(c for c in cases if c['id']=='icma_helpers.quarterly.SemiAnnual.0.04125.convert')
    for name,zeros in [('positive',[D('.017'),D('.029')]),('negative',[D('-.009'),D('-.012')])]:
        r=clone(base);inputs=r['inputs'];hs=[clone(short['request']['operation']['input']['helper']),clone(base_h)]
        for h in hs:h['quote_kind']['input']['inverse'].update(lower=-.5,upper=.5)
        cash=[refs[short['id']]['observed'],e];pillars=[h['pillar'] for h in hs];origin=hs[0]['settlement']
        grid=[(D(0),zeros[0])]+[(D((day(p)-day(origin)).days)/365,z) for p,z in zip(pillars,zeros)]
        expected={}
        for h,old in zip(hs,cash):
            dirty=curve_price(h,old['cashflows'],grid)
            y=bisect(lambda rate:D(quote_price(h,old['cashflows'],D(old['accrued_per100']),rate)['dirty_per100']),dirty)
            multiplier={'DecimalRate':D(1),'PercentRate':D(100),'BasisPoints':D(10000)}[h['quote']['unit']]
            h['quote']['value']=float(y*multiplier)
            observed=quote_price(h,old['cashflows'],D(old['accrued_per100']),y)
            expected[h['id']]=dict(observed=observed,model=observed,native_model=str(y*multiplier))
        key=dict(id='ICMA.MULTI.'+name,version='fixture.v1');curve=dict(key=key,currency='USD',role=dict(kind='Government',benchmark_id='ICMA.GOV'),reference_date=origin,time_basis='Act365Fixed',representation=dict(ZeroRates=dict(compounding='Continuous')),interpolation='Linear',extrapolation='None',dependencies=[],calibration='Piecewise',source=dict(Helpers=[h['id'] for h in hs]))
        recipe=dict(curve=curve,pillars=pillars,helpers=[dict(kind='Bond',input=h) for h in hs],bootstrap=dict(lower_parameter=-.08,upper_parameter=.15,max_iterations=128),basis_model=None,overlap='Reject')
        pins=r['operation']['input']['calendars'];graph=dict(rates=[],credit=[],cash=[dict(recipe=recipe,calendars=[dict(kind='bond',pins=pins) for _ in hs])],rate_context=None,controls=dict(jacobian_step=1e-6,minimum_singular_ratio=1e-10,svd_iterations=1000))
        r['operation']=dict(kind='calibration',input=graph,action=dict(kind='build'))
        answer=dict(parameters={key['id']:list(map(str,zeros))},helpers=expected,build_order=[key])
        for mode in ['piecewise','reversed','global']:
            request=clone(r);entry=request['operation']['input']['cash'][0]
            if mode=='reversed':entry['recipe']['helpers'].reverse();entry['calendars'].reverse();entry['recipe']['curve']['source']['Helpers'].reverse()
            if mode=='global':
                entry['recipe']['bootstrap']=None;entry['recipe']['curve']['calibration']=dict(Global=dict(initial_zeros=[.02,.025],lower_zero=-.08,upper_zero=.15,quote_scales=[1,1],weights=[1,1],objective_tolerance=1e-12,max_iterations=128))
            rows.append(dict(id='icma_helpers.multi.'+name+'.'+mode,request=request,expected_error=None,expected=clone(answer)))
    return dict(certified=False,precision=60,qualification='Separately predeclared complete Decimal60 two-pillar nonflat positive/negative source-order/reversed/piecewise/global original yield-quote fit plus actual delayed payment/include/exclude/ex-date/accrued ownership. No original input/answer/domain/bound changes.',cases=rows)
if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');a=p.parse_args();v=controls();path=ROOT/'tests/fixtures/library/icma-helpers-boundaries.json';data=(json.dumps(v,indent=2)+'\n').encode()
    if a.write:
        if path.exists():raise ValueError('Refusing to overwrite independently declared boundaries')
        path.write_bytes(data)
    elif path.read_bytes()!=data:raise ValueError('Original boundary control changed')
    print(json.dumps(dict(cases=len(v['cases']),certified=False)))
