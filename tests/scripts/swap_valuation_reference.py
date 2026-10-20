"""Independent Decimal-60 swap oracle. No Convex/QuantLib imports or execution.

Literal contractual/accrual/reset/observation dates come from the independently
reviewed CVX-018 input fixtures. This oracle prices those timelines with actual
dated zero grids, published observations and independent profile integration.
`check` is read-only. QuantLib comparison remains a separate, explicit gate.
"""
from pathlib import Path
from datetime import date
from decimal import Decimal, localcontext, ROUND_HALF_EVEN, ROUND_HALF_UP, ROUND_DOWN
import hashlib
import json
import sys

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'crates/convex-rates/tests/fixtures/swaps.json'
TARGET = ROOT / 'crates/convex-rates/tests/fixtures/valuation.json'
D = Decimal
def day(s): return date.fromisoformat(s)
def number(value): return format(value, '.30f')
def fraction(a,b,dc):
    if dc not in ('Act360','Act365Fixed'): raise ValueError('undeclared oracle day count')
    return D((b-a).days) / (360 if dc=='Act360' else 365)
def profile(entries,at):
    return D(next(e['value'] for e in entries if day(e['start'])<=at<day(e['end'])))
def integral(leg,components,a,b,rate=None):
    if b<=a: return D(0)
    cuts=sorted({a,b}|{day(e['start']) for p in [leg['notionals'],*components] for e in p if a<day(e['start'])<b})
    result=D(0)
    for left,right in zip(cuts,cuts[1:]):
        n=profile(leg['notionals'],left)
        values=[profile(p,left) for p in components]
        coupon=values[0] if rate is None else values[0]*rate+values[1]
        result+=n*coupon*fraction(left,right,leg['day_count'])
    return result
def unit(leg,a,b):
    return integral(leg,[[{'start':a.isoformat(),'end':b.isoformat(),'value':'1'}]],a,b)
def rounding(value,rule):
    if rule=='Unrounded': return value
    kind,scale=next(iter(rule.items()))
    return value.quantize(D(1).scaleb(-scale),rounding={'NearestEven':ROUND_HALF_EVEN,'MidpointAwayFromZero':ROUND_HALF_UP,'TowardZero':ROUND_DOWN}[kind])
def curve(terms,reference,projection=False,negative=False):
    key=terms['floating']['projection_curve'] if projection else terms['collateral']['discount_curve']
    role={'kind':'Projection','index_id':terms['floating'].get('resets',{}).get('index_id',terms['floating'].get('index_id'))} if projection else {'kind':'Discount','collateral_currency':terms['currency']}
    points=[('2023-10-01','0.037' if projection else '0.025'),('2024-07-01','0.041' if projection else '0.028'),('2026-01-01','0.044' if projection else '0.032')]
    if negative: points=[(s,'-0.005' if projection else '-0.01') for s,_ in points]
    return dict(key=key,role=role,currency=terms['currency'],reference_date=reference,time_basis='Act365Fixed',representation={'ZeroRates':{'compounding':'Continuous'}},interpolation='Linear',extrapolation='None',dependencies=[],calibration='Direct',source={'Nodes':[dict(pillar={'Date':s},quote=dict(value=float(v),unit='DecimalRate')) for s,v in points]})
def discount(curve,at):
    nodes=[(day(n['pillar']['Date']),D(str(n['quote']['value']))) for n in curve['source']['Nodes']]
    reference=day(curve['reference_date'])
    if at==reference: return D(1)
    if not nodes[0][0]<=at<=nodes[-1][0]: raise ValueError('oracle query outside pinned domain')
    for (a,za),(b,zb) in zip(nodes,nodes[1:]):
        if a<=at<=b:
            zero=za+(zb-za)*D((at-a).days)/D((b-a).days)
            return (-zero*fraction(reference,at,curve['time_basis'])).exp()
    raise ValueError('oracle interpolation gap')
def forward(curve,a,b,dc): return (discount(curve,a)/discount(curve,b)-1)/fraction(a,b,dc)
def compound(observations,rates,dc,cutoff=None):
    result=D(1)
    for o in observations:
        a,b=map(day,o['weight'])
        if cutoff is not None: b=min(b,cutoff)
        if b>a: result*=1+rates[o['observation']]*fraction(a,b,dc)
    return result
def reference(case,at,negative=False):
    terms=case['terms']; floating=terms['floating']; fixed=terms['fixed']; leg=floating['leg']
    curves=[curve(terms,'2023-10-01',negative=negative),curve(terms,'2023-10-01',projection=True,negative=negative)]
    anchor=discount(curves[0],at); sign=D(-1 if terms['side']=='PayFixed' else 1)
    rates={}; fixings={}; fixed_rows=[]; float_rows=[]
    fixed_pv=D(0); float_pv=D(0); annuity=D(0); float_annuity=D(0); spread_pv=D(0); fixed_accrued=D(0); float_accrued=D(0)
    for p in case['expected']['fixed']:
        payment=day(p['payment']); a,b=map(day,p['accrual'])
        if payment<at: continue
        amount=rounding(integral(fixed['leg'],[fixed['rates']],a,b),fixed['leg']['rounding'])*sign
        ratio=discount(curves[0],payment)/anchor
        fixed_pv+=amount*ratio; annuity+=unit(fixed['leg'],a,b)*ratio
        accrued=rounding(integral(fixed['leg'],[fixed['rates']],a,min(b,at)),fixed['leg']['rounding'])*sign
        fixed_accrued+=accrued
        fixed_rows.append(dict(payment=p['payment'],amount=number(amount),discount_ratio=number(ratio),pv=number(amount*ratio)))
    for p in case['expected']['floating']:
        payment=day(p['payment']); a,b=map(day,p['accrual'])
        if payment<at: continue
        observations=p['resets'] if floating['kind']=='term' else p['observations']
        for o in observations:
            observed=o['observation']
            if observed in rates: continue
            if day(observed)<=at:
                rates[observed]=D(case['rates'].get(observed,case.get('constant_rate')))
                stamp=observed+'T08:00:00Z'
                index=floating['resets']['index_id'] if floating['kind']=='term' else floating['index_id']
                fixings[observed]=dict(index_id=index,observation_date=observed,rate=str(rates[observed]),published_at=stamp,known_at=stamp,revision=1,status='Final',source_id='fixture')
            else:
                start,end=map(day,o['rate']) if floating['kind']=='term' else (day(observed),day(o['rate_end']))
                dc=floating['resets']['projection_day_count'] if floating['kind']=='term' else leg['day_count']
                rates[observed]=forward(curves[1],start,end,dc)
        if floating['kind']=='term':
            amount=D(0); accrued=D(0)
            for o in observations:
                left,right=map(day,o['accrual']); components=[floating['profiles']['gearings'],floating['profiles']['spreads']]
                amount+=integral(leg,components,left,right,rates[o['observation']])
                accrued+=integral(leg,components,left,min(right,at),rates[o['observation']])
        else:
            n=profile(leg['notionals'],a); gearing=profile(floating['profiles']['gearings'],a)
            amount=n*gearing*(compound(observations,rates,leg['day_count'])-1)+integral(leg,[floating['profiles']['spreads']],a,b)
            cutoff=min(b,at)
            accrued=D(0) if cutoff<=a else n*gearing*(compound(observations,rates,leg['day_count'],cutoff)-1)+integral(leg,[floating['profiles']['spreads']],a,cutoff)
        amount=rounding(amount,leg['rounding'])*-sign; accrued=rounding(accrued,leg['rounding'])*-sign
        ratio=discount(curves[0],payment)/anchor
        float_pv+=amount*ratio; float_annuity+=unit(leg,a,b)*ratio; spread_pv+=integral(leg,[floating['profiles']['spreads']],a,b)*ratio; float_accrued+=accrued
        float_rows.append(dict(payment=p['payment'],amount=number(amount),discount_ratio=number(ratio),pv=number(amount*ratio)))
    par=None
    if annuity>0 and float_annuity>0 and fixed['leg']['rounding']=='Unrounded' and leg['rounding']=='Unrounded' and fixed['leg']['negative_coupons']=='Allow' and leg['negative_coupons']=='Allow':
        unsigned_fixed=fixed_pv*sign; unsigned_float=float_pv*-sign
        par=dict(fixed_rate=number(unsigned_float/annuity),floating_spread=number((unsigned_fixed-unsigned_float+spread_pv)/float_annuity),fixed_rate_shift=number((unsigned_float-unsigned_fixed)/annuity),floating_spread_shift=number((unsigned_fixed-unsigned_float)/float_annuity),fixed_annuity=number(annuity),floating_annuity=number(float_annuity))
    if not fixed_rows and not float_rows: curves=[]
    expected=dict(pv=number(fixed_pv+float_pv),fixed_pv=number(fixed_pv),floating_pv=number(float_pv),fixed_accrued=number(fixed_accrued),floating_accrued=number(float_accrued),par=par,fixed=fixed_rows,floating=float_rows)
    return dict(id=case['id']+'/'+at.isoformat()+('/negative' if negative else ''),source_case=case['id'],valuation_date=at.isoformat(),known_at=at.isoformat()+'T23:59:59Z',curves=curves,fixings=list(fixings.values()),expected=expected)
def document():
    source=json.loads(SOURCE.read_text()); cases=[]
    for case in source['cases']:
        at=day('2024-04-07') if case['terms']['floating']['kind']=='overnight' else day('2024-07-15' if case['id']=='amortized-stepped-multiple-resets' else '2024-03-15')
        with localcontext() as context:
            context.prec=60; cases.append(reference(case,at))
    extra=[('regular-eom','2024-01-15',False),('ois-shift','2024-04-01',False),('regular-eom','2024-08-02',False),('regular-eom','2024-08-05',False),('regular-eom','2024-03-15',True)]
    for name,at,negative in extra:
        case=next(c for c in source['cases'] if c['id']==name)
        with localcontext() as context: context.prec=60; cases.append(reference(case,day(at),negative))
    return dict(schema_version=1,qualification='independent_manual_oracle_not_ore_quantlib_capture',certified=False,engine='Python datetime and Decimal, 60 digits; literal independently pinned timelines; no native library imports',source=dict(path=str(SOURCE.relative_to(ROOT)),sha256=hashlib.sha256(SOURCE.read_bytes()).hexdigest()),tolerances=dict(money_absolute='1e-7',rate_absolute='1e-12',discount_ratio_absolute='1e-13',date='exact',justification='New scoped synthetic cases with million-unit notional: independent Decimal-60 vs finite binary curve projection. Existing evidence/tolerances unchanged.'),cases=cases)
if __name__=='__main__':
    mode=sys.argv[1] if len(sys.argv)>1 else 'check'
    candidate=document()
    if mode=='generate': TARGET.write_text(json.dumps(candidate,indent=2)+'\n')
    elif mode=='check':
        if json.loads(TARGET.read_text())!=candidate: raise ValueError('independent valuation fixture drift')
    else: raise ValueError('check or generate required')
    print(json.dumps(dict(status='passed',mode=mode,cases=len(candidate['cases']),qualification=candidate['qualification'],certified=False)))
