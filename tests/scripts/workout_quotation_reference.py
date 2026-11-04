#!/usr/bin/env python3
"""Independent Decimal(60) fixed-workout controls; no candidate/library imports."""
import argparse
import copy
from datetime import date, timedelta
from decimal import Decimal, localcontext, ROUND_HALF_EVEN
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / 'tests/fixtures/library/workout-reference.json'
D = Decimal
HOLIDAYS = {date(2027, 6, 15), date(2027, 6, 21)}


def day(s):
    return date.fromisoformat(s)


def business(d):
    return d.weekday() < 5 and d not in HOLIDAYS


def offset(d, specification):
    kind, n = next(iter(specification.items()))
    if kind == 'CalendarDays':
        return d + timedelta(days=n)
    step = 1 if n >= 0 else -1
    for _ in range(abs(n)):
        d += timedelta(days=step)
        while not business(d):
            d += timedelta(days=step)
    return d


def adjust(d, rule):
    if rule == 'Unadjusted':
        return d
    result = d
    step = -1 if rule == 'Preceding' else 1
    while not business(result):
        result += timedelta(days=step)
    if rule == 'ModifiedFollowing' and result.month != d.month:
        return adjust(d, 'Preceding')
    return result


def number(v):
    return format(v, 'f')


def rounding(v, rule):
    if rule == 'Unrounded':
        return v
    scale = rule['NearestEven']
    return v.quantize(D(1).scaleb(-scale), rounding=ROUND_HALF_EVEN)


def fraction(start, end, basis):
    if basis == 'Act365Fixed':
        return D((end - start).days) / 365
    if basis == 'Act360':
        return D((end - start).days) / 360
    if basis == 'Thirty360E':
        return D((end.year-start.year)*360+(end.month-start.month)*30+min(end.day,30)-min(start.day,30))/360
    raise ValueError(basis)


def profile(terms, name, d):
    return next(D(p['value']) for p in terms['profiles'][name] if day(p['start']) <= d < day(p['end']))


def coupon(terms, start, end):
    cuts = {start, end}
    for name in ('coupon', 'notional'):
        for p in terms['profiles'][name]:
            if start < day(p['start']) < end:
                cuts.add(day(p['start']))
    cuts = sorted(cuts)
    return rounding(sum((profile(terms,'notional',a)*profile(terms,'coupon',a)*fraction(a,b,terms['day_count'])
                         for a,b in zip(cuts,cuts[1:])),D(0)), terms['rounding'])


def original_cash(terms, settlement):
    rows = []
    start = day(terms['schedule']['start_date'])
    maturity = day(terms['schedule']['end_date'])
    while start < maturity:
        end = date(start.year+1, start.month, start.day)
        payment = adjust(offset(end, terms['dates']['payment_delay']),terms['dates']['payment_adjustment'])
        if payment > settlement:
            rows.append(dict(kind='coupon',effective=end.isoformat(),date=payment.isoformat(),amount=number(coupon(terms,start,end))))
        start = end
    for a,b in zip(terms['profiles']['notional'],terms['profiles']['notional'][1:]):
        effective=day(b['start']); amount=D(a['value'])-D(b['value'])
        payment=adjust(offset(effective,terms['dates']['payment_delay']),terms['dates']['payment_adjustment'])
        if payment > settlement:
            rows.append(dict(kind='principal',effective=effective.isoformat(),date=payment.isoformat(),amount=number(rounding(amount,terms['rounding']))))
    payment=adjust(offset(maturity,terms['dates']['payment_delay']),terms['dates']['payment_adjustment'])
    if payment > settlement:
        rows.append(dict(kind='principal',effective=maturity.isoformat(),date=payment.isoformat(),
                         amount=number(rounding(D(terms['profiles']['notional'][-1]['value'])*D(terms['redemption_multiplier']),terms['rounding']))))
    return sorted(rows,key=lambda r:(r['date'],r['kind']))


def workout(terms, settlement, selection):
    original=original_cash(terms,settlement)
    if selection['kind']=='maturity':
        return dict(cash=original,dates=None,outstanding=None,redemption=None,accrued_cash=None,settlement_cash=None)
    provision=terms['exercise'][selection['provision']]; exercise=day(selection['date'])
    notice=offset(exercise,provision['notice_offset'])
    payment=adjust(offset(exercise,provision['settlement_lag']),provision['settlement_adjustment'])
    outstanding=profile(terms,'notional',exercise)
    redemption=outstanding*D(provision['price_per_100'])/100
    start=day(terms['schedule']['start_date'])
    while date(start.year+1,start.month,start.day) <= exercise:
        start=date(start.year+1,start.month,start.day)
    accrued=coupon(terms,start,exercise) if exercise>start and provision['accrued']=='pay_accrued' else D(0)
    cash= [r for r in original if day(r['effective'])<=exercise]
    total=rounding(redemption+accrued,terms['rounding'])
    cash.append(dict(kind='principal',effective=exercise.isoformat(),date=payment.isoformat(),amount=number(total-accrued)))
    if accrued:
        cash.append(dict(kind='coupon',effective=exercise.isoformat(),date=payment.isoformat(),amount=number(accrued)))
    return dict(cash=sorted(cash,key=lambda r:(r['date'],r['kind'])),
                dates=dict(notice=notice.isoformat(),exercise=exercise.isoformat(),settlement=payment.isoformat()),
                outstanding=number(outstanding),redemption=number(redemption),accrued_cash=number(accrued),settlement_cash=number(total))


def dirty(cash, settlement, y, convention, face):
    result=D(0)
    for r in cash:
        t=fraction(settlement,day(r['date']),convention['day_count']); c=convention['compounding']
        if c=='Continuous':
            df=(-y*t).exp()
        elif c=='Simple':
            df=1/(1+y*t)
        else:
            n=D({'Annual':1,'SemiAnnual':2,'Quarterly':4}[c]); df=(-n*t*(1+y/n).ln()).exp()
        result+=D(r['amount'])*df
    return result/face*100


def solve(cash, settlement, target, convention, face):
    low,high=D('-.01'),D('.25')
    for _ in range(180):
        mid=(low+high)/2
        if dirty(cash,settlement,mid,convention,face)>target:
            low=mid
        else:
            high=mid
    return (low+high)/2


def generate():
    template=json.loads((ROOT/'tests/fixtures/library/cases.json').read_bytes())
    bond=copy.deepcopy(next(c['request']['operation']['bond'] for c in template['cases'] if c['id']=='bond.yield'))
    t=bond['terms']; first='2026-01-01'; maturity='2029-01-01'
    t.update(issue_date=first,face_value='1000',day_count='Act365Fixed',redemption_multiplier='1.05',
             schedule=dict(start_date=first,end_date=maturity,frequency='Annual',end_of_month=False,stub_type='ShortLast',first_regular_date=None,penultimate_date=None))
    t['dates'].update(accrual_adjustment='Unadjusted',payment_adjustment='Following',payment_delay={'CalendarDays':10},ex_coupon=None)
    t['profiles']={
        'coupon':[dict(start=first,end='2027-04-01',value='0.04'),dict(start='2027-04-01',end=maturity,value='0.06')],
        'notional':[dict(start=first,end='2027-01-01',value='1000'),dict(start='2027-01-01',end='2028-01-01',value='700'),dict(start='2028-01-01',end=maturity,value='400')],
        'gearing':[dict(start=first,end=maturity,value='0')],'spread':[dict(start=first,end=maturity,value='0')]}
    call=dict(right='call',style=dict(style='european',date='2027-06-15'),price_per_100='101.25',notice_offset={'BusinessDays':-7},
              settlement_lag={'BusinessDays':5},settlement_adjustment='Following',accrued='pay_accrued',protection_end=None)
    put=dict(right='put',style=dict(style='bermudan',dates=['2027-07-15','2028-03-01']),price_per_100='99.5',notice_offset={'CalendarDays':-10},
             settlement_lag={'CalendarDays':3},settlement_adjustment='ModifiedFollowing',accrued='pay_accrued',protection_end=None)
    american=copy.deepcopy(call); american['style']=dict(style='american',start='2027-06-20',end='2027-06-30'); american['protection_end']='2027-06-22'
    t['exercise']=[call,put,american]
    calendar=dict(pin=bond['calendars']['payment'],holidays=sorted(d.isoformat() for d in HOLIDAYS),weekend_days=[5,6])
    context=dict(reference_date='2026-05-04',settlement='2026-05-06',on_payment='Exclude',notice=dict(kind='unnotified',earliest_notice='2026-05-05'))
    cases=[]
    for comp in ('Annual','SemiAnnual','Continuous','Simple'):
        for y in ('0.04125','-0.0075'):
            for selection in (dict(kind='maturity'),dict(kind='exercise',provision=0,date='2027-06-15'),dict(kind='exercise',provision=1,date='2027-07-15')):
                convention=dict(day_count='Act365Fixed',compounding=comp)
                prepared=workout(t,day(context['settlement']),selection)
                price=dirty(prepared['cash'],day(context['settlement']),D(y),convention,D(t['face_value']))
                cases.append(dict(id=f'workout.reference.{comp}.{y}.{selection["kind"]}.{selection.get("provision",0)}',bond=copy.deepcopy(bond),context=copy.deepcopy(context),
                                  selection=selection,convention=convention,decimal_yield=y,dirty_per100=number(price),expected=prepared))
    for name,exercise in [('before_principal','2026-12-30'),('principal_boundary','2027-01-01'),('delayed_claims','2027-01-02'),('included_accrued','2027-06-15'),('rounded','2027-06-15')]:
        b=copy.deepcopy(bond); p=b['terms']['exercise'][0]; p['style']=dict(style='european',date=exercise)
        p.update(notice_offset={'CalendarDays':-2},settlement_lag={'CalendarDays':0},settlement_adjustment='Unadjusted')
        if name=='included_accrued': p['accrued']='included_in_price'
        if name=='rounded': b['terms']['rounding']={'NearestEven':2}; p['price_per_100']='101.251234'
        selection=dict(kind='exercise',provision=0,date=exercise); convention=dict(day_count='Act360',compounding='Quarterly')
        prepared=workout(b['terms'],day(context['settlement']),selection)
        price=dirty(prepared['cash'],day(context['settlement']),D('.035'),convention,D(b['terms']['face_value']))
        cases.append(dict(id='workout.reference.'+name,bond=b,context=copy.deepcopy(context),selection=selection,convention=convention,
                          decimal_yield='0.035',dirty_per100=number(price),expected=prepared))
    # Every independently enumerated call/put date is inverted to ONE observed price.
    convention=dict(day_count='Act365Fixed',compounding='SemiAnnual'); observed=D('103.125')
    selections=[dict(kind='maturity'),dict(kind='exercise',provision=0,date='2027-06-15')]
    selections += [dict(kind='exercise',provision=1,date=d) for d in put['style']['dates']]
    selections += [dict(kind='exercise',provision=2,date=date(2027,6,d).isoformat()) for d in range(22,31)]
    all_quotes=[]
    for selection in selections:
        prepared=workout(t,day(context['settlement']),selection)
        y=solve(prepared['cash'],day(context['settlement']),observed,convention,D(t['face_value']))
        all_quotes.append(dict(selection=selection,expected=prepared,decimal_yield=number(y)))
    eligible=[i for i,q in enumerate(all_quotes) if q['selection']['kind']=='maturity' or t['exercise'][q['selection']['provision']]['right']=='call']
    return dict(schema_version=1,certified=False,qualification='Independent Decimal(60) supplied fixed-cash composition and dated discount/inversion controls; no market/Street/model/host/production certificate.',
                declared_bounds=dict(cash_absolute='0.0000000001',price_per100='0.00000001',decimal_yield='0.0000000001'),
                calendars=[calendar],cases=cases,all_dates=dict(bond=bond,context=context,convention=convention,dirty_per100=number(observed),quotes=all_quotes,
                                                             selected_index=min(eligible,key=lambda i:D(all_quotes[i]['decimal_yield']))))


def main():
    parser=argparse.ArgumentParser(description=__doc__); parser.add_argument('command',choices=['write','check'],nargs='?',default='check')
    args=parser.parse_args()
    with localcontext() as ctx:
        ctx.prec=60; content=(json.dumps(generate(),indent=2,sort_keys=True)+'\n').encode()
    if args.command=='write':
        if OUTPUT.exists(): raise ValueError('Original independent controls already declared; refusing overwrite')
        OUTPUT.write_bytes(content)
    elif OUTPUT.read_bytes()!=content: raise ValueError('Independent workout controls changed')
    print(json.dumps(dict(status='passed',cases=len(json.loads(content)['cases']),all_date_quotes=len(json.loads(content)['all_dates']['quotes']),
                          sha256=hashlib.sha256(content).hexdigest(),certified=False)))


if __name__=='__main__': main()
