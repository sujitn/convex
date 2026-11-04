"""Independent Decimal60 full-product and analytic-join controls.

Fictional exponential forward convergence, not regulatory Smith-Wilson.
Original dated cash/profile sums are independent Python references. No native
execution supplies quotes, answers, start choices, admissibility or tolerances.
"""
from copy import deepcopy
from datetime import date, timedelta
from decimal import Decimal as D, getcontext
from pathlib import Path
import argparse
import json
import cash_calibration_reference as cash
import product_parametric_reference as models

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/curves/product-ufr.json'
REF = '2026-10-07'
LLP = '2031-10-07'
PIN = dict(id='UFR.FICTIONAL', version=7)
ASOF = '2026-10-07T12:00:00Z'
CONTROLS = dict(jacobian_step=1e-6, minimum_singular_ratio=1e-10, svd_iterations=1000)

def span(on):
    return D((date.fromisoformat(on) - date.fromisoformat(REF)).days) / 365

def following(on):
    on = date.fromisoformat(on)
    while on.weekday() >= 5:
        on += timedelta(days=1)
    return on.isoformat()

def base_values(base, t, left=True):
    source = base['source']
    if 'FlatForward' in source:
        nodes = source['FlatForward']['nodes']
        times = [span(n['pillar']['Date']) for n in nodes]
        zeros = [D(str(n['quote']['value'])) for n in nodes]
        if t == 0:
            return zeros[0], (zeros[1]*times[1] - zeros[0]*times[0])/(times[1]-times[0])
        i = next(i for i in range(1, len(times)) if times[i] >= t)
        if not left and t == times[i] and i < len(times)-1:
            i += 1
        f = (zeros[i]*times[i] - zeros[i-1]*times[i-1])/(times[i]-times[i-1])
        return (zeros[i-1]*times[i-1] + f*(t-times[i-1]))/t, f
    m = source['Parametric']['parameters']
    tau = D(str(m.get('tau', m.get('tau1'))))
    def load(decay):
        x = t / decay
        e = (-x).exp()
        return (D(1) if t == 0 else (1-e)/x), e, x*e
    a, e, xe = load(tau)
    z = D(str(m['beta0'])) + D(str(m['beta1']))*a + D(str(m['beta2']))*(a-e)
    f = D(str(m['beta0'])) + D(str(m['beta1']))*e + D(str(m['beta2']))*xe
    if m['kind'] == 'svensson':
        a, e, xe = load(D(str(m['tau2'])))
        z += D(str(m['beta3']))*(a-e)
        f += D(str(m['beta3']))*xe
    return z, f

def joined(base, parameters, t):
    T = span(LLP)
    if t <= T:
        return base_values(base, t)
    zT, fT = base_values(base, T)
    u, a = D(str(parameters['ufr'])), D(str(parameters['alpha']))
    s = t - T
    e = (-a*s).exp()
    return (T*zT + u*s + (fT-u)*(1-e)/a)/t, u+(fT-u)*e

def discount(base, parameters, on):
    t = span(on)
    return (-joined(base, parameters, t)[0]*t).exp()

def build():
    cases = []
    for family in ['flat_forward', 'nelson_siegel', 'svensson']:
        for negative in [False, True]:
            for owner in ['rate', 'government', 'repo']:
                name = f'{family}.{owner}.{"negative" if negative else "positive"}'
                key = dict(id=name, version='fictional.tail.v1')
                base_key = dict(id=name+'.base', version='fictional.immutable.base.v7')
                role = (dict(kind='Discount', collateral_currency='USD') if owner=='rate' else
                        dict(kind='Government', benchmark_id='FICTIONAL.BENCHMARK') if owner=='government' else
                        dict(kind='Repo', collateral_id='FICTIONAL.COLLATERAL'))
                method = {'flat_forward':'FlatForward', 'nelson_siegel':'NelsonSiegel', 'svensson':'Svensson'}[family]
                base = dict(key=base_key, role=role, currency='USD', reference_date=REF,
                            time_basis='Act365Fixed', representation=dict(ZeroRates=dict(compounding='Continuous')),
                            interpolation=method, extrapolation='None', dependencies=[], calibration='Direct')
                if family == 'flat_forward':
                    base['source'] = dict(FlatForward=dict(nodes=[
                        dict(pillar=dict(Date=on), quote=dict(value=z-(.06 if negative else 0), unit='DecimalRate'))
                        for on, z in [(REF,.025), ('2028-10-07',.028), (LLP,.032), ('2034-10-07',.020)]]))
                else:
                    model = dict(kind=family, beta0=-.015 if negative else .04, beta1=-.012, beta2=.02)
                    model.update(dict(tau=1.7) if family=='nelson_siegel' else dict(beta3=-.012,tau1=1.7,tau2=4.0))
                    base['source'] = dict(Parametric=dict(parameters=model, horizon=dict(Date='2034-10-07'), observations=[]))
                actual = dict(ufr=-.008 if negative else .042, alpha=.16)
                initial = dict(ufr=actual['ufr']+.001, alpha=.144)
                helpers, calendars, expected, pillars, scales = [], [], [], [], []
                for i, maturity in enumerate(['2033-10-07', '2046-10-07']):
                    end = following(maturity)
                    identity = name+f'.quote.{i}'
                    unit = ['PercentRate', 'BasisPoints'][i]
                    scale = {'PercentRate':100, 'BasisPoints':10000}[unit]
                    rate = (1/discount(base, actual, end)-1)/cash.span(REF, end, 360)
                    quote = dict(value=float(rate*scale), unit=unit)
                    if owner == 'rate':
                        h = dict(id=identity, pillar=end, quote=quote, tolerance=1e-10*scale,
                                 terms=dict(kind='money_market', terms=dict(kind='Deposit', start_date=REF,
                                            maturity_date=end,day_count='Act360'), curve=key,calendar=PIN))
                    elif owner == 'repo':
                        h = dict(kind='Repo', input=dict(id=identity, quote_version=11+i,
                                 collateral_id='FICTIONAL.COLLATERAL',calendar=PIN,start_date=REF,end_date=end,
                                 day_count='Act360',quote=quote,tolerance=1e-10*scale))
                        calendars.append(dict(kind='repo', pin=PIN))
                    else:
                        terms = cash.terms(REF, maturity, coupon='.03')
                        terms['calendars'] = {k: PIN['id'] for k in terms['calendars']}
                        terms['settlement_lag'] = dict(BusinessDays=0)
                        split = cash.months(REF, 6)
                        terms['profiles']['coupon'] = cash.prof(REF, split, '.03')+cash.prof(split, maturity, '.045')
                        terms['profiles']['notional'] = cash.prof(REF, split, '5000')+cash.prof(split, maturity, '3000')
                        terms['redemption_multiplier'] = '1.01'
                        flows = cash.cash(terms, REF, [])
                        price = cash.dirty(flows, REF, lambda on: discount(base, actual, on))
                        quote = dict(value=float(price), unit='PricePer100')
                        h = dict(kind='Bond', input=dict(id=identity,quote_version=11+i,pillar=end,terms=terms,
                                 trade_date=REF,settlement=REF,on_payment='Exclude',credit_identity=None,
                                 quote_kind=dict(kind='CleanPrice' if i else 'DirtyPrice'),quote=quote,tolerance=1e-8))
                        calendars.append(dict(kind='bond', pins={k: PIN for k in terms['calendars']}))
                        scale = 100
                    helpers.append(h)
                    pillars.append(end)
                    scales.append(scale)
                    expected.append(dict(date=end, native_quote=quote))
                settings = dict(base=base_key,last_liquid_point=dict(Date=LLP),initial=initial,
                                lower=dict(ufr=-.1,alpha=.02),upper=dict(ufr=.1,alpha=1.0),
                                parameter_scales=[.01,.1],quote_scales=scales,weights=[1,1],
                                max_iterations=256,objective_tolerance=1e-12)
                curve = dict(key=key,role=role,currency='USD',reference_date=REF,time_basis='Act365Fixed',
                             representation=base['representation'],interpolation=method,
                             extrapolation=dict(UfrConvergence=initial),dependencies=[base_key],
                             source=dict(Helpers=[name+f'.quote.{i}' for i in range(2)]),
                             calibration=dict(Ufr=settings))
                recipe = dict(curve=curve,pillars=pillars,helpers=helpers,bootstrap=None)
                rates, cash_inputs = [], []
                if owner == 'rate':
                    recipe.update(overnight_projection=None, overlap_policy='Reject')
                    rates = [dict(recipe=recipe,quote_versions=[11,12],futures_calendar_stamps=[None,None])]
                else:
                    recipe.update(basis_model=None,overlap='Reject')
                    cash_inputs = [dict(recipe=recipe,calendars=calendars)]
                graph = dict(rates=rates,credit=[],cash=cash_inputs,controls=CONTROLS,
                             rate_context=dict(as_of=ASOF,cashflows=dict(on_payment='Include',fixings=dict(
                                 valuation_date=REF,known_at=ASOF,unpublished='Forecast',final_only=True))))
                inputs = dict(calendars=[dict(pin=PIN,holidays=[],weekend_days=[5,6])],curves=[base],
                              fixings=[],publications=[],helper_calendars=[],calibration=graph)
                values = []
                T = span(LLP)
                for t in [D(1), T-D('.000001'), T, T+D('.000001'), T+3, T+12, T+70]:
                    z, f = joined(base, actual, t)
                    values.append(dict(tenor=str(t),zero=str(z),forward_left=str(f),forward_right=str(f),
                                       discount_factor=str((-z*t).exp())))
                # At the LLP the join takes the actual left interval, not the
                # continuing base's other-sided forward.
                base_right = base_values(base, T, left=False)[1]
                cases.append(dict(id=name,parameters=actual,expected=expected,controls=values,
                                  original_base_right_at_llp=str(base_right),inputs=inputs))
    return dict(schema_version=1,certified=False,
                qualification='Independent fictional Decimal60 exponential-tail and full dated cash controls; no native financial answers or independent approval.',
                tolerances=dict(ufr_absolute=1e-7,alpha_absolute=1e-5,zero_absolute=1e-8,
                                forward_absolute=1e-8,discount_absolute=1e-8,join_absolute=1e-13),cases=cases)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    content = json.dumps(build(), indent=2)+'\n'
    if args.check:
        if DEST.read_text() != content:
            raise SystemExit('Frozen independent full-product UFR bytes differ')
        print('18 independent graphs,36 complete product quotes and126 analytic points passed')
    else:
        if DEST.exists():
            raise SystemExit('Refusing to overwrite independent controls')
        DEST.write_text(content)
