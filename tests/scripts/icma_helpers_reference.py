"""Independent Decimal60 full original cash/ICMA/helper/fit/risk declarations.

Reads preserved manual controls, never Convex output or a candidate executable.
New inputs, native units, domains and all bounds are fixed before execution.
"""
import argparse
from copy import deepcopy as clone
from decimal import Decimal as D, getcontext
import hashlib
import json
from pathlib import Path
from icma_reference import day, fraction, discount

getcontext().prec = 60
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'tests/fixtures/library'
SOURCES = ['icma-qualified.json', 'icma-reference.json', 'icma-additional-reference.json',
           'icma-cash-components.json', 'icma-entitlement-reference.json']
MULTIPLIER = {'DecimalRate': D(1), 'PercentRate': D(100), 'BasisPoints': D(10000)}

def bisect(function, target, lower=D('-.5'), upper=D('.5')):
    a, b = function(lower) - target, function(upper) - target
    if a * b > 0: raise ValueError('Independent original closed bracket has no root')
    for _ in range(170):
        mid = (lower + upper) / 2
        if function(mid) > target: lower = mid
        else: upper = mid
    return (lower + upper) / 2

def quote_price(h, cash, accrued, rate):
    i = h['quote_kind']['input']; settle = day(h['settlement'])
    times = {c['date']: fraction(settle, day(c['date']), i['reference_context']['periods'],
             {'Annual': 1, 'SemiAnnual': 2, 'Quarterly': 4, 'Monthly': 12}[i['convention']['frequency']]) for c in cash}
    pv = sum((D(c['amount']) * discount(rate, times[c['date']], i['convention']['compounding']) for c in cash), D(0))
    dirty = pv / D(h['terms']['face_value']) * 100
    return dict(cashflows=cash, times={k: str(v) for k, v in times.items()}, present_value=str(pv),
                dirty_per100=str(dirty), clean_per100=str(dirty-accrued), accrued_per100=str(accrued),
                decimal_yield=str(rate), consideration=str(dirty*D(i['position']['quantity'])/100))

def curve_price(h, cash, grid):
    def zero(time):
        if time <= grid[0][0]: return grid[0][1]
        for (a, za), (b, zb) in zip(grid, grid[1:]):
            if a <= time <= b: return za + (zb-za)*(time-a)/(b-a)
        raise ValueError('Independent full model cash is outside horizon')
    settle = day(h['settlement'])
    return sum((D(c['amount'])*(-zero(D((day(c['date'])-settle).days)/365)*D((day(c['date'])-settle).days)/365).exp()
                for c in cash), D(0)) / D(h['terms']['face_value']) * 100

def corpus():
    raw = json.loads((DEST/SOURCES[0]).read_bytes())['cases']
    refs = {}
    for file in [SOURCES[1], SOURCES[2], SOURCES[4]]:
        refs.update({c['id']: c['expected'] for c in json.loads((DEST/file).read_bytes())['cases']})
    components = {c['id']: c['expected'] for c in json.loads((DEST/SOURCES[3]).read_bytes())['cases']}
    cases, answers, originals = [], [], []
    def add(id, request, answer=None, error=None):
        cases.append(dict(id=id, request=clone(request), expected_error=error))
        if error is None:
            if answer is None: raise ValueError('Every success needs a complete independent answer')
            answers.append(dict(id=id, expected=clone(answer)))
    def make(source, unit, number):
        op = source['request']['operation']; security = op['input']['security']; t = clone(security['bond']['terms'])
        pins = {name: dict(id='ICMA.HELPER.'+name.upper(), version=7+n) for n, name in enumerate(t['calendars'])}
        t['calendars'] = {name: pin['id'] for name, pin in pins.items()}
        reference = clone(op['input']['reference_context']); reference['security'] = dict(id='ICMA.HELPER.'+str(number), version=17)
        convention = clone(op['convention']); convention['max_reference_evaluations'] = 10000
        h = dict(id=reference['security']['id'], quote_version=23, pillar=max(c['date'] for c in refs[source['id']]['cashflows']),
                 terms=t, trade_date=op['cash']['settlement'], settlement=op['cash']['settlement'], on_payment=op['cash']['on_payment'],
                 credit_identity=None, quote_kind=dict(kind='ContextualIcmaYield', input=dict(
                     sources=dict(security=reference['security'], quote_revision=dict(id='ICMA.OBSERVATION.'+str(number), version=23),
                                  quotation_policy=op['input']['quotation_policy'], calendars=pins),
                     reference_context=reference, convention=convention,
                     inverse=dict(lower=-.15, upper=.15, price_tolerance=1e-8, parameter_tolerance=1e-12, max_iterations=128),
                     position=op['input']['position'], max_cash_evaluations=8000000)),
                 quote=dict(value=float(D(str(op['action']['decimal_yield']))*MULTIPLIER[unit]), unit=unit), tolerance=1e-8)
        inputs = clone(source['request']['inputs']); inputs['curves'] = []; inputs['calibration'] = None
        original_calendar = inputs['calendars'][0]
        inputs['calendars'] = [dict(pin=pin, holidays=clone(original_calendar['holidays']), weekend_days=[5, 6]) for pin in pins.values()]
        expected = clone(refs[source['id']]); expected['cashflows'] = clone(components.get(source['id'], expected)['cashflows'])
        return h, pins, inputs, expected
    def recipe(h, key, role=None):
        return dict(curve=dict(key=key, currency='USD', role=role or dict(kind='Government', benchmark_id='ICMA.GOV'),
                    reference_date=h['settlement'], time_basis='Act365Fixed', representation=dict(ZeroRates=dict(compounding='Continuous')),
                    interpolation='Linear', extrapolation='None', dependencies=[], calibration='Piecewise', source=dict(Helpers=[h['id']])),
                    pillars=[h['pillar']], helpers=[dict(kind='Bond', input=clone(h))], bootstrap=dict(lower_parameter=-.08, upper_parameter=.15, max_iterations=128),
                    basis_model=None, overlap='Reject')
    def direct(r, values):
        c = clone(r['curve']); c.update(calibration='Direct', dependencies=[])
        c['source'] = dict(Nodes=[dict(pillar=dict(Date=date), quote=dict(value=float(value), unit='DecimalRate'))
            for date, value in zip([c['reference_date']]+r['pillars'], [values[0]]+values)])
        return c
    selected = [c for c in raw if c['expected_error'] is None and c['request']['operation']['cash']['kind']=='plain'
                and c['request']['operation']['action']['kind']=='price']
    for n, source in enumerate(selected):
        unit = list(MULTIPLIER)[n % 3]; h, pins, inputs, prior = make(source, unit, n)
        observed = quote_price(h, prior['cashflows'], D(prior['accrued_per100']), D(prior['decimal_yield']))
        base = dict(schema_version=1, inputs=inputs, operation=dict(kind='helper', input=dict(kind='cash_quote', helper=h, calendars=pins, reference_date=h['settlement'])))
        id = 'icma_helpers.'+source['id'].removeprefix('icma.')
        add(id+'.convert', base, dict(observed=observed)); originals.append((h, pins, inputs, observed))
        key = dict(id='ICMA.DIRECT.'+str(n), version='fixture.v1'); r = recipe(h, key)
        z = D('-.0045') if n % 2 else D('.023')
        horizon = D((day(h['pillar'])-day(h['settlement'])).days)/365
        dirty = curve_price(h, observed['cashflows'], [(D(0), z), (horizon, z)])
        y = bisect(lambda rate: D(quote_price(h, observed['cashflows'], D(observed['accrued_per100']), rate)['dirty_per100']), dirty, D('-.15'), D('.15'))
        implied = quote_price(h, observed['cashflows'], D(observed['accrued_per100']), y)
        supplied = clone(inputs); supplied['curves'] = [direct(r, [z])]
        reprice = dict(schema_version=1, inputs=supplied, operation=dict(kind='helper', input=dict(kind='cash_reprice', input=dict(recipe=r, calendars=[dict(kind='bond', pins=pins)]))))
        add(id+'.reprice', reprice, dict(observed=observed, model=implied, native_model=str(y*MULTIPLIER[unit]), within_tolerance=False))
    # Actual split retirement allocation and payment/ex boundaries, independently
    # derived from the original complete cash controls rather than candidate output.
    stepped = next(row for row in originals if row[0]['terms']['profiles']['notional'][-1]['value']=='700')
    h, pins, inputs, observed = clone(stepped); h['terms']['principal_prices']=[dict(effective_date='2024-10-15', multiplier='1.03', amount='100')]
    flows = clone(observed['cashflows']); principal = next(c for c in flows if c['kind']=='principal' and D(c['amount'])==300)
    principal['amount']='200'; flows.append(dict(date=principal['date'],kind='principal',amount='103')); flows.sort(key=lambda c:(c['date'],c['kind'],D(c['amount'])))
    e = quote_price(h, flows, D(observed['accrued_per100']), D(observed['decimal_yield']))
    add('icma_helpers.allocation.convert', dict(schema_version=1, inputs=inputs, operation=dict(kind='helper',input=dict(kind='cash_quote',helper=h,calendars=pins,reference_date=h['settlement']))), dict(observed=e))
    # Complete two-helper flat government + residual-basis dependency graph.
    # The full original curve callback uses actual cash, not sampled yield nodes.
    graph_bases=[]
    for label, original_index in [('positive',0),('negative',1),('stepped',48)]:
        h, pins, inputs, observed = clone(originals[original_index]); h['quote_kind']['input']['inverse'].update(lower=-.5, upper=.5)
        gkey=dict(id='ICMA.GOV.'+label,version='fixture.v1'); gov=recipe(h,gkey)
        z=bisect(lambda rate: curve_price(h,observed['cashflows'],[(D(0),rate),(D(5),rate)]),D(observed['dirty_per100']),D('-.08'),D('.15'))
        issuer=clone(h); issuer['id']+='-issuer'; issuer['quote_kind']['input']['sources']['security']['id']=issuer['id']; issuer['quote_kind']['input']['reference_context']['security']['id']=issuer['id']
        issuer['credit_identity']=dict(currency='USD',entity_id='Synthetic.ICMA.Issuer',instrument_id=issuer['id'],instrument_version=17,maturity=issuer['terms']['schedule']['end_date'],reference=dict(id='ICMA.LEGAL',version=19),seniority='SeniorUnsecured')
        iy=D(observed['decimal_yield'])+D('.0125'); issuer['quote']['value']=float(iy*MULTIPLIER[issuer['quote']['unit']])
        ie=quote_price(issuer,observed['cashflows'],D(observed['accrued_per100']),iy)
        iz=bisect(lambda rate:curve_price(issuer,observed['cashflows'],[(D(0),rate),(D(5),rate)]),D(ie['dirty_per100']),D('-.08'),D('.15'))
        bkey=dict(id='ICMA.BASIS.'+label,version='fixture.v1'); basis=recipe(issuer,bkey,dict(kind='IssuerSpread',entity_id='Synthetic.ICMA.Issuer',model_id='reference_zero_spread_v1'))
        basis['curve'].update(dependencies=[gkey],representation='IssuerSpreads');basis['basis_model']=dict(kind='ReferenceZeroSpread',reference=gkey,role=dict(kind='Government',benchmark_id='ICMA.GOV'))
        graph=dict(rates=[],credit=[],cash=[dict(recipe=basis,calendars=[dict(kind='bond',pins=pins)]),dict(recipe=gov,calendars=[dict(kind='bond',pins=pins)])],rate_context=None,controls=dict(jacobian_step=1e-6,minimum_singular_ratio=1e-10,svd_iterations=1000))
        base=dict(schema_version=1,inputs=inputs,operation=dict(kind='calibration',input=graph,action=dict(kind='build')))
        ex=dict(parameters={gkey['id']:[str(z)],bkey['id']:[str(iz-z)]},helpers={h['id']:dict(observed=observed,model=observed,native_model=str(D(observed['decimal_yield'])*MULTIPLIER[h['quote']['unit']])),issuer['id']:dict(observed=ie,model=ie,native_model=str(iy*MULTIPLIER[issuer['quote']['unit']]))},build_order=[gkey,bkey])
        add('icma_helpers.graph.'+label+'.build',base,ex);graph_bases.append(base)
        global_=clone(base)
        for entry in global_['operation']['input']['cash']:
            entry['recipe']['bootstrap']=None;entry['recipe']['curve']['calibration']=dict(Global=dict(initial_zeros=[0.02],lower_zero=-.08,upper_zero=.15,quote_scales=[1.0],weights=[1.0],objective_tolerance=1e-12,max_iterations=128))
        add('icma_helpers.graph.'+label+'.global',global_,ex)
        key=dict(factor=dict(basis='market_quote_recalibrated',convention_version=41,currency='USD',family='interest_rate',model_version='contextual-icma-helper-v1',quote_type='yield',target=dict(kind='curve',curve_id=gkey['id']),tenor='actual-cash'),native_unit=h['quote']['unit'],response='rebuild_dependents',unit='currency_per_bp',coordinate=dict(kind='quote',curve=gkey,helper_id=h['id'],quote_version=23))
        scenario=clone(base);scenario['operation']['action']=dict(kind='scenario',key=key,delta=.0001)
        shocked=quote_price(h,observed['cashflows'],D(observed['accrued_per100']),D(observed['decimal_yield'])+D('.0001'))
        nz=bisect(lambda rate:curve_price(h,observed['cashflows'],[(D(0),rate),(D(5),rate)]),D(shocked['dirty_per100']),D('-.08'),D('.15'))
        sx=clone(ex);sx['parameters']={gkey['id']:[str(nz)],bkey['id']:[str(iz-nz)]};sx['helpers'][h['id']]=dict(observed=shocked,model=shocked,native_model=str((D(observed['decimal_yield'])+D('.0001'))*MULTIPLIER[h['quote']['unit']]))
        add('icma_helpers.graph.'+label+'.scenario',scenario,sx)
        context=dict(settlement=h['settlement'],on_payment=h['on_payment'],fixings=dict(valuation_date=h['settlement'],known_at=h['settlement']+'T12:00:00Z',unpublished='Forecast',final_only=True))
        product=dict(kind='bond',bond=dict(terms=h['terms'],calendars=pins),context=context,projection=None,method=dict(kind='discount_spread',curve=gkey,role=dict(kind='government',benchmark_id='ICMA.GOV'),decimal_spread=0.0))
        risk=clone(base);risk['operation']['action']=dict(kind='product_risk',key=key,product=product,position=h['quote_kind']['input']['position'],snapshot_id='synthetic-ICMA-helpers',settings=dict(bump=.0001,policy_version=1,stability=dict(absolute=.05,relative=.0001)))
        values=[D(quote_price(h,observed['cashflows'],D(observed['accrued_per100']),D(observed['decimal_yield'])+delta)['present_value'])*D('-2.5') for delta in map(D,['0','.0001','-.0001','.00005','-.00005'])]
        re=dict(zip(['base_pv','plus_pv','minus_pv','half_plus_pv','half_minus_pv'],map(str,values)));re.update(value=str((values[1]-values[2])/2),half_value=str(values[3]-values[4]))
        add('icma_helpers.graph.'+label+'.risk',risk,dict(risk=re))
    # Source and native model/fit failures, retaining the unchanged predeclared
    # brackets/tolerances of every positive input and all original source files.
    base=cases[1]['request']
    def helper(r):return r['operation']['input']['input']['recipe']['helpers'][0]['input']
    mutations=[
        ('empty_context',lambda h:h['quote_kind']['input']['reference_context'].update(periods=[]),'invalid_input'),
        ('context_revision',lambda h:h['quote_kind']['input']['convention']['reference_context'].update(version=99),'invalid_input'),
        ('security_revision',lambda h:h['quote_kind']['input']['sources']['security'].update(version=99),'invalid_input'),
        ('quote_revision',lambda h:h.update(quote_version=99),'invalid_input'),
        ('policy_revision',lambda h:h['quote_kind']['input']['sources']['quotation_policy'].update(version=99),'invalid_input'),
        ('calendar_revision',lambda h:h['quote_kind']['input']['sources']['calendars']['payment'].update(version=99),'invalid_input'),
        ('frequency',lambda h:h['quote_kind']['input']['convention'].update(frequency='Annual'),'invalid_input'),
        ('reference_work',lambda h:h['quote_kind']['input']['convention'].update(max_reference_evaluations=1),'resource_limit'),
        ('cash_work',lambda h:h['quote_kind']['input'].update(max_cash_evaluations=10),'resource_limit'),
        ('model_cash_work',lambda h:h['quote_kind']['input'].update(max_cash_evaluations=100),'resource_limit'),
        ('inverse_work',lambda h:h['quote_kind']['input']['inverse'].update(max_iterations=1),'no_convergence'),
        ('out_of_bracket',lambda h:h['quote'].update(value=.4),'invalid_input'),
        ('wrong_unit',lambda h:h['quote'].update(unit='PricePer100'),'invalid_input'),
        ('pillar',lambda h:h.update(pillar=h['terms']['schedule']['end_date']),'invalid_input'),
        ('inverse_controls',lambda h:h['quote_kind']['input']['inverse'].update(parameter_tolerance=0),'invalid_input'),
        ('mixed_sign',lambda h:h['terms']['profiles']['coupon'][0].update(value='-0.5'),'unsupported'),
        ('wrong_original_face',lambda h:h['quote_kind']['input']['position'].update(reference_quantity='700'),'invalid_input'),
        ('currency',lambda h:h['quote_kind']['input']['position'].update(currency='EUR'),'invalid_input'),
        ('optional',lambda h:h['terms'].update(exercise=clone(raw[0]['request']['operation']['input']['security']['bond']['terms']['exercise']) or [dict(accrued='pay_accrued',notice_offset={'CalendarDays':-5},price_per_100='101.25',protection_end=None,right='call',settlement_adjustment='Following',settlement_lag={'BusinessDays':5},style={'date':'2024-06-17','style':'european'})]),'invalid_input'),
    ]
    for name,change,error in mutations:
        r=clone(base);change(helper(r));add('icma_helpers.invalid.'+name,r,error=error)
    r=clone(base);r['inputs']['calendars'].pop();add('icma_helpers.invalid.missing_calendar',r,error='missing_input')
    r=clone(base);r['inputs']['curves'][0]['source']['Nodes'][1]['quote']['value']=.8;add('icma_helpers.invalid.model_no_root',r,error='no_root')
    r=clone(base);h=helper(r);h['terms']['profiles']['coupon'][0]['value']='0';h['terms']['profiles']['spread'][0]['value']='0';h['terms']['redemption_multiplier']='0.000001';h['quote_kind']['input']['inverse']['price_tolerance']=1;add('icma_helpers.invalid.non_unique',r,error='non_unique')
    r=clone(graph_bases[0]);r['operation']['input']['cash'][0]['recipe']['helpers'][0]['input']['quote_kind']['input']['reference_context']['periods']=[]
    r['operation']['input']['cash'][1]['recipe']['helpers'][0]['input']['quote_kind']['input']['inverse'].update(lower=.04,upper=.05)
    add('icma_helpers.invalid.late_context_before_fit',r,error='invalid_input')
    r=clone(graph_bases[0]);
    for entry in r['operation']['input']['cash']:entry['recipe']['helpers'][0]['input']['quote_kind']['input']['max_cash_evaluations']=40000000
    add('icma_helpers.invalid.total_cash_work',r,error='resource_limit')
    r=clone(graph_bases[0]);
    for entry in r['operation']['input']['cash']:entry['recipe']['helpers'][0]['input']['quote_kind']['input']['convention']['max_reference_evaluations']=5000000
    add('icma_helpers.invalid.total_reference_work',r,error='resource_limit')
    r=clone(graph_bases[0]);r['operation']['input']['cash'][1]['recipe']['bootstrap'].update(max_iterations=1);add('icma_helpers.invalid.fit_work',r,error='no_convergence')
    r=clone(next(c['request'] for c in cases if c['id']=='icma_helpers.graph.positive.scenario'));r['operation']['action']['key']['factor']['convention_version']=99;add('icma_helpers.invalid.risk_policy',r,error='invalid_input')
    return dict(certified=False,cases=cases),dict(certified=False,precision=60,tolerances=dict(price_cash_invoice='1e-8',time='1e-12',yield_parameter='1e-9',fit_parameter='1e-8',risk='1e-7',relative='0'),source_reads={f:hashlib.sha256((DEST/f).read_bytes()).hexdigest() for f in SOURCES},cases=answers)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--write',action='store_true');args=p.parse_args()
    values=corpus()
    for file,value in zip(['icma-helpers.json','icma-helpers-reference.json'],values):
        path=DEST/file;data=(json.dumps(value,indent=2)+'\n').encode()
        if args.write:
            if path.exists():raise ValueError('Refusing to overwrite original declared controls')
            path.write_bytes(data)
        elif path.read_bytes()!=data:raise ValueError('Independent original control changed: '+file)
    print(json.dumps(dict(requests=len(values[0]['cases']),successes=len(values[1]['cases']),failures=len(values[0]['cases'])-len(values[1]['cases']),certified=False)))
