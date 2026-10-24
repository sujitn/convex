"""Independent Decimal(60) product controls, frozen before candidate execution.

Reuse independent dated cash/profile sums; no native measurements determine any
quote, parameter or bound. All inputs are fictional. --check recomputes bytes.
"""
from copy import deepcopy
from datetime import date
from decimal import Decimal as D, getcontext
from pathlib import Path
import argparse
import json
import cash_calibration_reference as cash

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/curves/product-parametric.json'
REF = '2026-10-07'
PIN = dict(id='PARAMETRIC.FICTIONAL', version=7)
ASOF = '2026-10-07T12:00:00Z'
CONTROLS = dict(jacobian_step=1e-6, minimum_singular_ratio=1e-10, svd_iterations=1000)

def span(on):
    return D((date.fromisoformat(on)-date.fromisoformat(REF)).days)/365

def zero(model, on):
    t = span(on)
    def loads(tau):
        if not t: return D(1), D(0)
        x = t / D(str(tau)); e = (-x).exp(); a = (1-e)/x
        return a, a-e
    a, b = loads(model.get('tau', model.get('tau1')))
    z = D(str(model['beta0'])) + D(str(model['beta1']))*a + D(str(model['beta2']))*b
    if model['kind'] == 'svensson':
        z += D(str(model['beta3']))*loads(model['tau2'])[1]
    return z

def df(model, on): return (-zero(model,on)*span(on)).exp()

def model_settings(model, scales):
    initial = deepcopy(model)
    initial['beta0'] += .001; initial['beta1'] -= .001; initial['beta2'] += .001
    if model['kind']=='svensson':
        initial['beta3'] -= .001; initial['tau1'] *= .9; initial['tau2'] *= 1.1
        lower=[-.2,-.2,-.2,-.2,.3,2.5]; upper=[.2,.2,.2,.2,2.4,8.0]
    else:
        initial['tau'] *= .9; lower=[-.2,-.2,-.2,.3]; upper=[.2,.2,.2,4.0]
    return dict(initial=initial,lower=lower,upper=upper,quote_scales=scales,
                weights=[1]*len(scales),max_iterations=256,objective_tolerance=1e-11)

def build():
    cases=[]
    for family in ['nelson_siegel','svensson']:
        dates = (['2027-04-07','2027-10-07','2029-10-08','2034-10-09'] if family=='nelson_siegel'
                 else ['2027-01-07','2027-04-07','2027-10-07','2028-10-09','2031-10-07','2036-10-07'])
        for negative in [False,True]:
            model=dict(kind=family,beta0=-.015 if negative else .04,beta1=-.012,beta2=.02)
            model.update(dict(tau=1.7) if family=='nelson_siegel' else dict(beta3=-.012,tau1=1.7,tau2=4.0))
            for owner in ['rate','government','repo']:
                name=f'{family}.{owner}.{"negative" if negative else "positive"}'
                key=dict(id=name,version='fictional.original.v1')
                helpers=[]; scales=[]; expected=[]; calendars=[]
                for i,end in enumerate(dates):
                    identity=f'{name}.quote.{i}'
                    unit=['DecimalRate','PercentRate','BasisPoints'][i%3]
                    scale={'DecimalRate':1,'PercentRate':100,'BasisPoints':10000}[unit]
                    rate=(1/df(model,end)-1)/cash.span(REF,end,360)
                    quote=dict(value=float(rate*scale),unit=unit)
                    if owner=='rate':
                        helper=dict(id=identity,pillar=end,terms=dict(kind='money_market',
                            terms=dict(kind='Deposit',start_date=REF,maturity_date=end,day_count='Act360'),
                            curve=key,calendar=PIN),quote=quote,tolerance=1e-10*scale)
                    elif owner=='repo':
                        helper=dict(kind='Repo',input=dict(id=identity,quote_version=11+i,
                            collateral_id='FICTIONAL.COLLATERAL',calendar=PIN,start_date=REF,end_date=end,
                            day_count='Act360',quote=quote,tolerance=1e-10*scale))
                        calendars.append(dict(kind='repo',pin=PIN))
                    else:
                        terms=cash.terms(REF,end,coupon='.03')
                        terms['calendars']={k:PIN['id'] for k in terms['calendars']}
                        terms['settlement_lag']=dict(BusinessDays=0)
                        split=cash.months(REF,6)
                        if split<end:
                            terms['profiles']['coupon']=cash.prof(REF,split,'.03')+cash.prof(split,end,'.045')
                            terms['profiles']['notional']=cash.prof(REF,split,'5000')+cash.prof(split,end,'3000')
                        terms['redemption_multiplier']='1.01'
                        flows=cash.cash(terms,REF,[])
                        price=cash.dirty(flows,REF,lambda on:df(model,on))
                        helper=dict(kind='Bond',input=dict(id=identity,quote_version=11+i,pillar=end,
                            terms=terms,trade_date=REF,settlement=REF,on_payment='Exclude',credit_identity=None,
                            quote_kind=dict(kind='CleanPrice' if i%2 else 'DirtyPrice'),
                            quote=dict(value=float(price),unit='PricePer100'),tolerance=1e-8))
                        quote=helper['input']['quote']; scale=100
                        calendars.append(dict(kind='bond',pins={k:PIN for k in terms['calendars']}))
                    helpers.append(helper); scales.append(scale)
                    expected.append(dict(date=end,zero=str(zero(model,end)),discount_factor=str(df(model,end)),native_quote=quote))
                role=(dict(kind='Discount',collateral_currency='USD') if owner=='rate' else
                      dict(kind='Government',benchmark_id='FICTIONAL.BENCHMARK') if owner=='government' else
                      dict(kind='Repo',collateral_id='FICTIONAL.COLLATERAL'))
                curve=dict(key=key,role=role,currency='USD',reference_date=REF,time_basis='Act365Fixed',
                    representation=dict(ZeroRates=dict(compounding='Continuous')),
                    interpolation='NelsonSiegel' if family=='nelson_siegel' else 'Svensson',
                    extrapolation='None',dependencies=[],source=dict(Helpers=[f'{name}.quote.{i}' for i in range(len(dates))]),
                    calibration=dict(Parametric=model_settings(model,scales)))
                recipe=dict(curve=curve,pillars=dates,helpers=helpers,bootstrap=None)
                rates=[]; cash_inputs=[]
                if owner=='rate':
                    recipe.update(overnight_projection=None,overlap_policy='Reject')
                    rates=[dict(recipe=recipe,quote_versions=[11+i for i in range(len(dates))],futures_calendar_stamps=[None]*len(dates))]
                else:
                    recipe.update(basis_model=None,overlap='Reject')
                    cash_inputs=[dict(recipe=recipe,calendars=calendars)]
                graph=dict(rates=rates,credit=[],cash=cash_inputs,controls=CONTROLS,
                    rate_context=dict(as_of=ASOF,cashflows=dict(on_payment='Include',fixings=dict(
                        valuation_date=REF,known_at=ASOF,unpublished='Forecast',final_only=True))))
                inputs=dict(calendars=[dict(pin=PIN,holidays=[],weekend_days=[5,6])],curves=[],
                            fixings=[],publications=[],helper_calendars=[],calibration=graph)
                cases.append(dict(id=name,model=model,expected=expected,inputs=inputs))
    return dict(schema_version=1,certified=False,qualification='Fictional Decimal(60) engineering controls; no native-generated financial expectations or independent approval.',
        source='https://www.bis.org/publications/paper-25-zero-coupon-yield-curves-technical-documentation.pdf',
        tolerances=dict(zero_absolute=1e-8,discount_absolute=1e-8,beta_absolute=1e-6,decay_absolute=1e-4),cases=cases)

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');args=parser.parse_args()
    content=json.dumps(build(),indent=2)+'\n'
    if args.check:
        if DEST.read_text()!=content: raise SystemExit('Frozen independent product control bytes differ')
        print('12 independent original product graphs and 60 dated quote/model controls passed')
    else:
        if DEST.exists(): raise SystemExit('Refusing to overwrite independent controls')
        DEST.write_text(content)
