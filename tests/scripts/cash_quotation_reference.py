#!/usr/bin/env python3
"""Declared synthetic Decimal(60) full-cash controls, without native execution."""
import argparse
import copy
import datetime as dt
from decimal import Decimal, localcontext
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CORPUS = ROOT / 'tests/fixtures/library/cash.json'
REFERENCE = ROOT / 'tests/fixtures/library/cash-reference.json'
D = Decimal
ORIGIN = '2024-01-01'
CONVENTION = dict(day_count='Act365Fixed', compounding='SemiAnnual')
SETTINGS = dict(lower=-0.1, upper=0.5, price_tolerance=1e-9, parameter_tolerance=1e-13, max_iterations=128)


def date(value):
    return dt.date.fromisoformat(value)


def discount(rate, days, convention):
    t = D(days) / (D(360) if convention['day_count'] == 'Act360' else D(365))
    comp = convention['compounding']
    if comp == 'Simple': return 1 / (1 + rate * t)
    if comp == 'Continuous': return (-rate * t).exp()
    n = D(dict(Annual=1, SemiAnnual=2, Quarterly=4, Monthly=12, Daily=365)[comp])
    return (1 + rate/n) ** (-n*t)


def cash(helper):
    """Independent annual, unadjusted profile/principal cash for declared controls."""
    terms = helper['terms']; start = date(terms['schedule']['start_date']); end = date(terms['schedule']['end_date'])
    settlement = date(helper['settlement']); face = D(terms['face_value'])
    coupon = terms['profiles']['coupon']; notional = terms['profiles']['notional']
    knots = [start] + [dt.date(y, 1, 1) for y in range(start.year+1, end.year+1) if dt.date(y, 1, 1) < end] + [end]
    flows=[]; accrued=D(0)
    for a,b in zip(knots,knots[1:]):
        amount=D(0); prefix=D(0)
        for cp in coupon:
            for np in notional:
                lo=max(a,date(cp['start']),date(np['start'])); hi=min(b,date(cp['end']),date(np['end']))
                if hi>lo:
                    scale=D(cp['value'])*D(np['value'])/365
                    amount += scale*D((hi-lo).days)
                    if settlement>lo: prefix += scale*D((min(hi,settlement)-lo).days)
        pay=b+dt.timedelta(days=terms['dates']['payment_delay']['CalendarDays'])
        if pay>settlement or pay==settlement and helper['on_payment']=='Include':flows.append((pay,amount))
        if a<settlement<b:accrued=prefix
    for left,right in zip(notional,notional[1:]):
        pay=date(left['end'])+dt.timedelta(days=terms['dates']['payment_delay']['CalendarDays'])
        if pay>settlement or pay==settlement and helper['on_payment']=='Include':flows.append((pay,D(left['value'])-D(right['value'])))
    last=end+dt.timedelta(days=terms['dates']['payment_delay']['CalendarDays'])
    if last>settlement or last==settlement and helper['on_payment']=='Include':
        flows.append((last,D(notional[-1]['value'])*D(terms['redemption_multiplier'])))
    return flows,accrued*100/face


def values(helper, convention):
    flows, accrued=cash(helper); face=D(helper['terms']['face_value']); settle=date(helper['settlement'])
    kind=helper['quote_kind']['kind']; quote=helper['quote']; unit=quote['unit']
    value=D(str(quote['value']))
    def price(y, conv):
        return sum((amount*discount(y,(day-settle).days,conv) for day,amount in flows),D(0))*100/face
    if kind=='CashflowYield':
        rate=value/(D(100) if unit=='PercentRate' else D(10000) if unit=='BasisPoints' else D(1))
        dirty=price(rate,helper['quote_kind']['convention'])
    elif kind=='CleanPrice':dirty=value+accrued
    elif kind=='DirtyPrice':dirty=value
    elif kind=='ParCouponRate':dirty=100+accrued
    elif kind=='BankDiscountYield':
        rate=value/(D(100) if unit=='PercentRate' else D(10000) if unit=='BasisPoints' else D(1))
        basis=D(360) if helper['quote_kind']['day_count']=='Act360' else D(365)
        dirty=100*(1-rate*D((flows[-1][0]-settle).days)/basis)
    else:raise ValueError(kind)
    lower=D(str(SETTINGS['lower'])); upper=D(str(SETTINGS['upper']))
    for _ in range(240):
        middle=(lower+upper)/2
        if price(middle,convention)>dirty:lower=middle
        else:upper=middle
    return dict(clean_per100=str(dirty-accrued),dirty_per100=str(dirty),accrued_per100=str(accrued),
                decimal_yield=str((lower+upper)/2),currency=helper['terms']['currency'])


def build():
    original={c['id']:c['request'] for c in json.loads((ROOT/'tests/fixtures/library/cases.json').read_bytes())['cases']}
    template=original['helper.cash_quote']; calendar=copy.deepcopy(template['inputs']['calendars'][0])
    calendar.update(holidays=[],weekend_days=[])
    inputs=dict(calendars=[calendar],calibration=None,curves=[],fixings=[],helper_calendars=[],publications=[])
    pins=copy.deepcopy(template['operation']['input']['calendars'])
    base=copy.deepcopy(template['operation']['input']['helper'])
    cases=[]; references=[]
    def helper(identity,years=2,rate=0.041275,currency='USD',settlement=ORIGIN,zero=False,stepped=False,delay=0):
        h=copy.deepcopy(base); t=h['terms']; end=f'{2024+years}-01-01' if years else '2024-03-01'
        h.update(id=identity,quote_version=11,pillar=(date(end)+dt.timedelta(days=delay)).isoformat(),trade_date=settlement,settlement=settlement,credit_identity=None)
        t.update(issue_date=ORIGIN,currency=currency,face_value='5000',day_count='Act365Fixed',settlement_lag={'CalendarDays':0},settlement_adjustment='Unadjusted')
        t['schedule'].update(start_date=ORIGIN,end_date=end,frequency='Annual',first_regular_date=None,penultimate_date=None)
        t['dates'].update(accrual_adjustment='Unadjusted',payment_adjustment='Unadjusted',payment_delay={'CalendarDays':delay},ex_coupon=None)
        t['profiles']={name:[dict(start=ORIGIN,end=end,value=value)] for name,value in (('coupon','0' if zero else '0.035'),('spread','0'),('gearing','0'),('notional','5000'))}
        if stepped:
            split='2025-01-01'; t['profiles']['coupon']=[dict(start=ORIGIN,end=split,value='0.03'),dict(start=split,end=end,value='0.055')]
            t['profiles']['notional']=[dict(start=ORIGIN,end=split,value='5000'),dict(start=split,end=end,value='3000')]
        h['quote_kind']=dict(kind='CashflowYield',convention=dict(CONVENTION,lower_yield=-0.1,upper_yield=0.5,max_iterations=128,price_tolerance=1e-9))
        h['quote']=dict(unit='DecimalRate',value=rate)
        return h
    def add(identity,operation,error=None,resources=inputs):
        cases.append(dict(id=identity,request=dict(schema_version=1,inputs=copy.deepcopy(resources),operation=copy.deepcopy(operation)),expected_error=error))
        return cases[-1]
    def quote(identity,h,action=None,conv=CONVENTION):
        operation=dict(kind='cash_security',input=dict(helper=h,calendars=pins),reference_date=h['trade_date'],convention=conv,settings=SETTINGS,action=action or dict(kind='quote'))
        row=add(identity,operation); expected=values(h,conv)
        if action:
            expected['consideration']=str(D(expected['dirty_per100'])*D(action['position']['quantity'])/100)
        references.append(dict(id=identity,expected=expected,price_absolute=1e-8,yield_absolute=1e-9,money_absolute=1e-6))
        return row
    position=dict(quantity='15000',reference_quantity='5000',quantity_unit='notional',currency='USD')
    for currency in ('USD','EUR','GBP'):
        for kind in ('CashflowYield','CleanPrice','DirtyPrice'):
            h=helper('synthetic.'+currency+'.'+kind,currency=currency,settlement='2024-06-01')
            initial=values(h,CONVENTION)
            if kind!='CashflowYield':h.update(quote_kind=dict(kind=kind),quote=dict(unit='PricePer100',value=float(initial['clean_per100' if kind=='CleanPrice' else 'dirty_per100'])))
            quote('cash.quote.'+currency+'.'+kind,h)
    for comp in ('Simple','Continuous','Annual','Quarterly','Monthly','Daily'):
        quote('cash.quote.compounding.'+comp,helper('synthetic.comp.'+comp),conv=dict(CONVENTION,compounding=comp))
    quote('cash.quote.act360',helper('synthetic.act360'),conv=dict(CONVENTION,day_count='Act360'))
    quote('cash.quote.negative',helper('synthetic.negative',rate=-0.01))
    quote('cash.quote.stepped_amortizing',helper('synthetic.stepped',years=3,stepped=True))
    quote('cash.quote.delayed',helper('synthetic.delayed',delay=10))
    quote('cash.quote.bill_yield',helper('synthetic.bill',years=0,zero=True,rate=-0.01))
    h=helper('synthetic.bank',years=0,zero=True); h.update(quote_kind=dict(kind='BankDiscountYield',day_count='Act360'),quote=dict(unit='PercentRate',value=3.5));quote('cash.quote.bank_discount',h)
    h=helper('synthetic.par');h.update(quote_kind=dict(kind='ParCouponRate'),quote=dict(unit='BasisPoints',value=350.0));quote('cash.quote.par',h)
    for identity,quantity in (('long','15000'),('short','-10000'),('zero','0')):
        quote('cash.invoice.'+identity,helper('synthetic.invoice',settlement='2024-06-01'),action=dict(kind='invoice',position=dict(position,quantity=quantity)))
    h=helper('synthetic.nonpar',years=3,stepped=True);h['terms']['redemption_multiplier']='1.02'
    quote('cash.invoice.nonpar',h,action=dict(kind='invoice',position=position))
    def government(identity,target,benchmarks,selection='LinearMaturity',conv=CONVENTION):
        securities=[dict(quote=dict(security=dict(id=h['id'],version=i+21),benchmark_id='synthetic.government',helper=h),calendars=pins) for i,h in enumerate(benchmarks)]
        operation=dict(kind='government_spread',target=dict(helper=target,calendars=pins),benchmarks=securities,reference_date=ORIGIN,
                       settings=dict(benchmark_id='synthetic.government',convention=conv,inverse=SETTINGS,selection=selection))
        row=add(identity,operation); tv=values(target,conv); bv=[values(h,conv) for h in benchmarks]
        end=date(target['terms']['schedule']['end_date']); days=[date(h['terms']['schedule']['end_date']) for h in benchmarks]
        if end in days:lo=days.index(end);hi=None;weight=D(0)
        else:
            lo=max((i for i,d in enumerate(days) if d<end),key=lambda i:days[i]);hi=min((i for i,d in enumerate(days) if d>end),key=lambda i:days[i])
            weight=D((end-days[lo]).days)/D((days[hi]-days[lo]).days)
        by=D(bv[lo]['decimal_yield']) if hi is None else D(bv[lo]['decimal_yield'])+weight*(D(bv[hi]['decimal_yield'])-D(bv[lo]['decimal_yield']))
        expected=dict(target=tv,benchmarks=bv,lower_index=lo,upper_index=hi,upper_weight=str(weight),benchmark_decimal_yield=str(by),basis_points=str((D(tv['decimal_yield'])-by)*10000))
        references.append(dict(id=identity,expected=expected,price_absolute=1e-8,yield_absolute=1e-9,spread_bp_absolute=1e-5))
        return row
    low=helper('synthetic.gov.low',years=1,rate=0.03);high=helper('synthetic.gov.high',years=3,rate=0.05)
    low['quote']=dict(unit='PercentRate',value=3.0);high['quote']=dict(unit='BasisPoints',value=500.0)
    target=helper('synthetic.target',rate=0.041275)
    government('government.linear',target,[high,low])
    government('government.linear.negative',helper('synthetic.target',rate=-0.012345),[helper('synthetic.low',years=1,rate=-0.02),helper('synthetic.high',years=3,rate=-0.01)])
    government('government.linear.stepped',helper('synthetic.target',years=3,stepped=True),[helper('synthetic.high',years=5,rate=0.05),helper('synthetic.low',years=1,rate=0.03)])
    government('government.linear.leap_delayed',helper('synthetic.target',years=4),[helper('synthetic.low',years=3,rate=0.03,delay=10),helper('synthetic.high',years=5,rate=0.05)])
    government('government.linear.requoted_annual',target,[high,low],conv=dict(CONVENTION,compounding='Annual'))
    government('government.linear.origin_boundary',low,[low,high])
    government('government.exact',target,[helper('synthetic.exact',rate=0.03)],'ExactMaturity')
    government('government.exact.negative',helper('synthetic.target',rate=-0.01),[helper('synthetic.exact',rate=-0.015)],'ExactMaturity')
    positives={r['id']:r for r in cases}
    def bad(identity,source,mutate,error='invalid_input'):
        row=copy.deepcopy(positives[source]);row.update(id=identity,expected_error=error);mutate(row['request']);cases.append(row)
    source='cash.quote.USD.CashflowYield'
    bad('cash.quote.missing_calendar',source,lambda r:r['operation']['input']['calendars']['payment'].update(version=999),'missing_input')
    bad('cash.quote.invalid_settlement',source,lambda r:r['operation']['input']['helper'].update(settlement='2024-06-02'))
    bad('cash.quote.future_trade',source,lambda r:r['operation'].update(reference_date=ORIGIN))
    bad('cash.quote.quote_unit',source,lambda r:r['operation']['input']['helper']['quote'].update(unit='PricePer100'))
    bad('cash.quote.native_price_limit',source,lambda r:r['operation']['input']['helper'].update(tolerance=0.0))
    bad('cash.quote.missing_icma',source,lambda r:r['operation']['convention'].update(day_count='ActActIcma'),'unsupported')
    bad('cash.quote.nonpositive_base',source,lambda r:r['operation']['settings'].update(lower=-2.0),'no_convergence')
    bad('cash.quote.no_root',source,lambda r:r['operation']['settings'].update(lower=0.1,upper=0.2),'no_root')
    bad('cash.quote.work_limit',source,lambda r:r['operation']['settings'].update(max_iterations=0))
    bad('cash.quote.price_tolerance',source,lambda r:r['operation']['settings'].update(price_tolerance=0.0))
    bad('cash.invoice.currency','cash.invoice.long',lambda r:r['operation']['action']['position'].update(currency='EUR'))
    bad('cash.invoice.reference','cash.invoice.long',lambda r:r['operation']['action']['position'].update(reference_quantity='100'))
    bad('cash.invoice.unit','cash.invoice.long',lambda r:r['operation']['action']['position'].update(quantity_unit='trade_copies'))
    bad('cash.invoice.quantity_limit','cash.invoice.long',lambda r:r['operation']['action']['position'].update(quantity='1000000000000001'))
    bad('cash.invoice.zero_still_checks','cash.invoice.zero',lambda r:r['operation']['input']['helper'].update(settlement='2024-06-02'))
    g='government.linear'
    bad('government.linear.empty',g,lambda r:r['operation'].update(benchmarks=[]))
    bad('government.linear.identity',g,lambda r:r['operation']['benchmarks'][0]['quote']['security'].update(id='synthetic.wrong'))
    bad('government.linear.benchmark',g,lambda r:r['operation']['benchmarks'][0]['quote'].update(benchmark_id='different'))
    bad('government.linear.currency',g,lambda r:r['operation']['benchmarks'][0]['quote']['helper']['terms'].update(currency='EUR'))
    bad('government.linear.settlement',g,lambda r:r['operation']['benchmarks'][0]['quote']['helper'].update(settlement='2024-01-02',trade_date='2024-01-02'))
    bad('government.linear.entitlement',g,lambda r:r['operation']['benchmarks'][0]['quote']['helper'].update(on_payment='Include'))
    bad('government.linear.duplicate',g,lambda r:r['operation']['benchmarks'].append(copy.deepcopy(r['operation']['benchmarks'][0])))
    bad('government.linear.missing_calendar',g,lambda r:r['operation']['benchmarks'][0]['calendars']['payment'].update(version=999),'missing_input')
    bad('government.linear.closed_domain',g,lambda r:r['operation'].update(benchmarks=[r['operation']['benchmarks'][0]]))
    bad('government.linear.bad_unselected',g,lambda r:r['operation']['benchmarks'].append(dict(quote=dict(security=dict(id='synthetic.unselected',version=90),benchmark_id='synthetic.government',helper=helper('synthetic.unselected',years=5,rate=0.6)),calendars=pins)))
    bad('government.exact.wrong_maturity','government.exact',lambda r:r['operation']['benchmarks'][0]['quote'].update(helper=helper('synthetic.exact',years=3,rate=0.03)))
    bad('government.exact.multiple','government.exact',lambda r:r['operation']['benchmarks'].append(copy.deepcopy(r['operation']['benchmarks'][0])))
    def underflow(r):
        h=helper('synthetic.underflow',years=150,zero=True)
        h.update(quote_kind=dict(kind='DirtyPrice'),quote=dict(unit='PricePer100',value=1e-300))
        r['operation']['input']['helper']=h
        r['operation']['settings'].update(lower=0.0,upper=10.0)
        r['operation']['action']['position'].update(quantity='0.0000000000000000000000000001')
    bad('cash.invoice.underflow','cash.invoice.long',underflow)
    def same_maturity(r):
        b=copy.deepcopy(r['operation']['benchmarks'][0]); b['quote']['security']['id']='synthetic.same_maturity';b['quote']['helper']['id']='synthetic.same_maturity'
        r['operation']['benchmarks'].append(b)
    bad('government.linear.duplicate_maturity',g,same_maturity)
    bad('government.linear.work_limit',g,lambda r:r['operation'].update(benchmarks=[copy.deepcopy(r['operation']['benchmarks'][0]) for _ in range(129)]),'resource_limit')
    # Typed economic guards above are structural-schema-valid. Discarded fields
    # and numeric cash are separate negative decoding checks in the Rust tests.
    corpus=dict(schema_version=1,certified=False,qualification='Declared synthetic full-term inputs; no actual market/certificate.',cases=cases)
    reference=dict(schema_version=1,certified=False,precision=60,provenance='Independent Python Decimal actual annual coupon/profile/principal/settlement cash, bisection yield and actual-date interpolation. Declared before native execution; no native output used.',cases=references)
    return corpus,reference


def documents():
    with localcontext() as context:
        context.prec=60
        corpus,reference=build()
        return {CORPUS:(json.dumps(corpus,sort_keys=True,separators=(',',':'))+'\n').encode(),REFERENCE:(json.dumps(reference,indent=2)+'\n').encode()}


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--write',action='store_true');args=parser.parse_args()
    for path,content in documents().items():
        if args.write:
            if path.exists():raise ValueError('Reference is immutable; refusing to overwrite '+str(path))
            path.write_bytes(content)
        elif path.read_bytes()!=content:raise ValueError('Declared inputs/reference drift: '+str(path))
    print(json.dumps(dict(status='passed',native_execution=False,reference_declared=True)))
