"""Independent Decimal(60) dated cash/carry, basket CTD and five-point risk.

Only existing supplied-input JSON shapes are read; no native answer is read or
executed. All new controls, bounds and scenarios are immutable before execution.
"""
import argparse
import calendar
import copy
import datetime as dt
from decimal import Decimal, localcontext, ROUND_HALF_UP
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = ROOT / 'tests/fixtures/library'
SOURCE = 'tests/fixtures/library/cases.json'
TARGET = HERE / 'delivery-hedge-reference.json'
D = Decimal
HOLIDAYS = {dt.date(2026, 11, 9), dt.date(2026, 12, 1)}
SETTLEMENT = dt.date(2026, 10, 8)
DELIVERY = dt.date(2026, 12, 8)


def serial(v):
    if isinstance(v, D): return format(v, 'f')
    if isinstance(v, dt.date): return v.isoformat()
    raise TypeError(type(v))


def date(s): return dt.date.fromisoformat(s)
def fraction(a, b, denominator=365): return D((b - a).days) / denominator
def following(d):
    while d.weekday() > 4 or d in HOLIDAYS: d += dt.timedelta(days=1)
    return d
def months(d, count):
    month = d.year * 12 + d.month - 1 + count
    year, month = divmod(month, 12); month += 1
    return dt.date(year, month, min(d.day, calendar.monthrange(year, month)[1]))
def stamp(identifier, version=1):
    return dict(id=identifier, version=version, source='independent engineering control',
        source_at='2026-10-07T10:00:00Z', available_at='2026-10-07T10:00:00Z',
        valid_until='2027-01-01T00:00:00Z', quality='synthetic',
        evidence='Full independent synthetic dated cash/carry; no market or execution certificate')
def pin(identifier, version=1): return dict(id=identifier, version=version)
def graph():
    return dict(rates=[], credit=[], cash=[], rate_context=None,
        controls=dict(jacobian_step=1e-6, minimum_singular_ratio=1e-10, svd_iterations=1000))
def position(quantity=1, currency='USD'):
    return dict(quantity=str(quantity), reference_quantity='1', quantity_unit='contracts', currency=currency)
def settings(bump='0.0002', absolute='0.001'):
    return dict(bump=float(bump), stability=dict(absolute=float(absolute), relative=1e-5), policy_version=1)
def key(kind, future, node=None, curve=None):
    currency = future['contract']['currency']
    if kind == 'discount':
        curve = curve or dict(id='D.DELIVERY', version='7')
        family, target, quote, basis, tenor = 'interest_rate', dict(kind='curve', curve_id=curve['id']), 'zero_rate', 'zero_node', 'parallel' if node is None else '4Y'
        coordinate = dict(kind='zero', curve=curve, node=node)
        native, unit = 'DecimalRate', 'currency_per_bp'
    else:
        member = future['members'][0 if kind.endswith('a') else 1]
        repo = kind.startswith('repo')
        source = member['financing']['stamp'] if repo else member['quote_stamp']
        family, target, quote, basis, tenor = ('interest_rate' if repo else 'cash_price'), dict(kind='instrument', instrument_id=member['instrument']['id']), ('repo_rate' if repo else 'cash_clean_price'), 'raw_quote', 'spot'
        coordinate = dict(kind='raw', input_id=source['id'], version=source['version'])
        native, unit = ('DecimalRate' if repo else 'PricePer100'), ('currency_per_bp' if repo else 'currency_per_price_point')
    return dict(factor=dict(family=family, target=target, currency=currency, tenor=tenor,
        quote_type=quote, basis=basis, convention_version=1, model_version='full-original-fixed-date-basket-v1'),
        coordinate=coordinate, native_unit=native, response='frozen_curves', unit=unit)


def pack(currency='USD', control='switch'):
    source = json.loads((ROOT / SOURCE).read_bytes())
    original = next(c['request'] for c in source['cases'] if c['id'] == 'bond_future.price')
    resources = copy.deepcopy(original['inputs'])
    resources['calendars'][0]['pin'] = pin('DELIVERY.CALENDAR', 3)
    resources['calendars'][0]['holidays'] = ['2026-11-09', '2026-12-01']
    resources['calendars'][1]['holidays'] = ['2026-11-09', '2026-12-01']
    resources['calendars'][1]['pin']['version'] = 2
    curve = resources['curves'][0]
    curve['currency'] = currency; curve['key'] = dict(id='D.DELIVERY', version='7')
    curve['role']['collateral_currency'] = currency
    curve['source']['Nodes'] = [dict(pillar=dict(Tenor=0.0), quote=dict(unit='DecimalRate', value=0.03)),
        dict(pillar=dict(Tenor=4.0), quote=dict(unit='DecimalRate', value=0.04)),
        dict(pillar=dict(Tenor=20.0), quote=dict(unit='DecimalRate', value=0.045))]
    future = copy.deepcopy(original['operation']['input'])
    fid = 'SYNTH.DELIVERY.' + currency
    c = future['contract']; c['instrument_id'] = fid; c['contract_code'] = 'SYNTHZ6'
    c['product_code'] = 'SYNTH'; c['exchange'] = 'ENGINEERING'; c['currency'] = currency
    c['stamp'] = stamp(fid, 4); c['daily_settlement']['calendar'] = pin('DELIVERY.CALENDAR', 3)
    c['settlement']['delivery']['calendar'] = pin('DELIVERY.CALENDAR', 3)
    c['settlement']['delivery']['basket'] = pin(fid + '.BASKET', 2)
    c['currency_per_price_point'] = '1000'; c['settlement']['contract_face'] = '100000'
    future['basket'].update(contract=pin(fid, 4), currency=currency, stamp=stamp(fid+'.BASKET', 2))
    future['calendar_stamp'] = stamp('DELIVERY.CALENDAR', 3)
    future['market'].update(contract=pin(fid, 4), stamp=stamp(fid+'.MARK', 5), price_points='100')
    future['selection'] = 'minimum_forward_price'
    members = []
    for i, maturity in enumerate(['2029-05-08', '2034-05-08', '2031-06-01']):
        m = copy.deepcopy(original['operation']['input']['members'][0]); identifier = 'SYNTH.BASKET.' + 'ABC'[i]
        m['instrument'] = pin(identifier, 2 + i); m['stamp'] = stamp(identifier, 2+i)
        m['quote_stamp'] = stamp(identifier+'.CASH', 10+i)
        t = m['bond']['terms']; start = '2025-05-08' if i < 2 else '2025-06-01'
        t['currency'] = currency; t['face_value'] = '350' if i < 2 else '250'; t['issue_date'] = start
        t['schedule'].update(start_date=start, end_date=maturity, end_of_month=False)
        t['dates']['payment_adjustment'] = 'Following'
        for binding in m['bond']['calendars'].values(): binding['version'] = 2
        for name in ['notional', 'gearing', 'spread']:
            t['profiles'][name] = [dict(start=start, end=maturity, value=t['face_value'] if name=='notional' else '0')]
        t['profiles']['coupon'] = [dict(start=start, end='2026-09-20', value='0.025'),
            dict(start='2026-09-20', end=maturity, value='0.06')]
        if i == 1:
            t['profiles']['coupon'][1]['end'] = '2028-01-03'
            t['profiles']['coupon'].append(dict(start='2028-01-03', end=maturity, value='0.031'))
        m['clean_price_per100'] = '94' if i < 2 else '150'
        m['financing'].update(instrument=m['instrument'], currency=currency, stamp=stamp(identifier+'.REPO', 20+i))
        m['own_cash']['stamp'] = stamp(identifier+'.OWN', 30+i)
        m['coupon_carry'] = dict(kind='reinvested', rate=dict(
            stamp=stamp(identifier+'.REINVEST', 40+i), start=future['settlement'], end='2026-12-31',
            decimal_rate='0.027', day_count='Act365Fixed'))
        if i == 2:
            m['financing']['repo_decimal'] = '-0.01'; m['financing']['haircut_fraction'] = '0'; m['own_cash'] = None
            m['coupon_carry'] = dict(kind='held_cash', evidence='Explicit uninvested interim coupon cash')
        members.append(m)
    if control == 'stable': members[1]['clean_price_per100'] = '110'
    if control == 'near_switch': members[1]['clean_price_per100'] = '94.005'
    if control == 'tie': members[1]['bond'] = copy.deepcopy(members[0]['bond'])
    if control == 'criteria':
        members[1]['financing']['repo_decimal'] = '-0.01'
        members[1]['clean_price_per100'] = '96'; members[1]['financing']['haircut_fraction'] = '0'
        members[1]['own_cash'] = None
    future['members'] = members
    future['basket']['bonds'] = [dict(instrument=m['instrument'], conversion_factor='0.9069' if i < 2 else '0.92') for i,m in enumerate(members)]
    product = dict(kind='bond_future', input=future,
        discount=dict(curve=curve['key'], stamp=stamp('D.DELIVERY', 7)), policy=dict(max_member_evaluations=576))
    return resources, product


def cash(member, curve, delta= D(0), node=None):
    terms = member['bond']['terms']; start = date(terms['schedule']['start_date']); end = date(terms['schedule']['end_date'])
    face = D(terms['face_value']); flows=[]; coupons=[]; accrued=D(0)
    def integrated(a, b):
        return sum((D(p['value'])*fraction(max(a, date(p['start'])), min(b, date(p['end'])))
            for p in terms['profiles']['coupon'] if max(a, date(p['start'])) < min(b, date(p['end']))), D(0)) * face
    cursor=start
    while cursor < end:
        stop=min(months(cursor, 6), end); pay=following(stop); amount=integrated(cursor, stop)
        if pay > SETTLEMENT:
            flows.append((pay, amount)); coupons.append((cursor, stop, pay, amount))
        if cursor < SETTLEMENT < stop: accrued = integrated(cursor, SETTLEMENT)
        cursor=stop
    flows.append((following(end), face))
    knots=curve['source']['Nodes']; reference=date(curve['reference_date'])
    def zero(at):
        t=fraction(reference, at)
        times=[D(str(k['pillar']['Tenor'])) for k in knots]
        values=[D(str(k['quote']['value'])) + (delta if node is None or i==node else D(0)) for i,k in enumerate(knots)]
        if t <= times[0]: return values[0]
        if t >= times[-1]: return values[-1]
        j=next(j for j in range(1,len(times)) if t<=times[j])
        return values[j-1]+(values[j]-values[j-1])*(t-times[j-1])/(times[j]-times[j-1])
    def df(at): return (-zero(at)*fraction(reference, at)).exp()
    pv=sum((amount*df(pay)/df(SETTLEMENT) for pay,amount in flows),D(0))
    dirty=pv/face*100; ai=accrued/face*100
    # Delivery accrued is independent of financial movements and uses all steps.
    ai_delivery=D(0)
    for a,b,pay,amount in coupons:
        if a < DELIVERY < b: ai_delivery=integrated(a,DELIVERY)/face*100
    return dict(pv=pv, dirty=dirty, clean=dirty-ai, accrued=ai, delivery_accrued=ai_delivery,
        coupons=[dict(start=a,end=b,payment=pay,currency_amount=amount,per100=amount/face*100) for a,b,pay,amount in coupons],
        cashflows=[dict(date=pay,amount=amount) for pay,amount in flows])


def valuation(product, resources, factor, delta):
    future=product['input']; curve=resources['curves'][0]; alternatives=[]; prices=[]
    coordinate=factor['coordinate']; raw=coordinate['kind']=='raw'
    consumed = not raw and coordinate.get('curve') == curve['key']
    move=delta if consumed else D(0); node=coordinate.get('node')
    for i,m in enumerate(future['members']):
        base=cash(m,curve); changed=cash(m,curve,move,node)
        repo=factor['factor']['quote_type']=='repo_rate' and raw and factor['factor']['target'].get('instrument_id')==m['instrument']['id']
        basis=factor['factor']['quote_type']=='cash_clean_price' and raw and factor['factor']['target'].get('instrument_id')==m['instrument']['id']
        clean=D(m['clean_price_per100'])+changed['clean']-base['clean']+(delta if basis else D(0))
        dirty=clean+base['accrued']; finance=m['financing']; haircut=D(finance['haircut_fraction'])
        tau=fraction(SETTLEMENT,DELIVERY,360 if finance['day_count']=='Act360' else 365)
        repo_rate=D(finance['repo_decimal'])+(delta if repo else D(0))
        repo_cost=dirty*(1-haircut)*repo_rate*tau
        own=D(0) if m['own_cash'] is None else dirty*haircut*D(m['own_cash']['decimal_rate'])*fraction(SETTLEMENT,DELIVERY)
        interim=[c for c in base['coupons'] if c['payment']<=DELIVERY]
        coupons=sum((c['per100'] for c in interim),D(0)); carried=D(0)
        for c in interim:
            amount=c['per100']
            if m['coupon_carry']['kind']=='reinvested': amount*=1+D(m['coupon_carry']['rate']['decimal_rate'])*fraction(c['payment'],DELIVERY)
            carried+=amount
        cf=D(future['basket']['bonds'][i]['conversion_factor']); market=D(future['market']['price_points']); mult=D(future['contract']['currency_per_price_point'])
        forward_clean=dirty+repo_cost+own-carried-base['delivery_accrued']; net=forward_clean-market*cf
        irr=(market*cf+base['delivery_accrued']+carried-dirty-own)/(dirty*(1-haircut)*tau)
        invoice=(market*cf*mult).quantize(D('.01'),rounding=ROUND_HALF_UP)+(base['delivery_accrued']*mult).quantize(D('.01'),rounding=ROUND_HALF_UP)
        alternatives.append(dict(instrument=m['instrument'],conversion_factor=cf,spot_clean_per100=clean,
            spot_dirty_per100=dirty,settlement_accrued_per100=base['accrued'],delivery_accrued_per100=base['delivery_accrued'],
            coupons_per100=coupons,coupon_delivery_value_per100=carried,repo_cost_per100=repo_cost,
            own_cash_cost_per100=own,forward_price_points=forward_clean/cf,gross_basis_per100=clean-market*cf,
            net_basis_per100=net,implied_repo_decimal=irr,delivery_invoice_per_contract=invoice,
            unrounded_basis_profit_per_contract=-net*mult,repo_rate=repo_rate))
        prices.append(dict(instrument=m['instrument'],baseline=base,scenario=changed,
            frozen_residual_basis_per100=D(m['clean_price_per100'])-base['clean'],scenario_clean_per100=clean))
    alternatives.sort(key=lambda a:(a['instrument']['id'],a['instrument']['version']))
    def score(a):
        return {'minimum_forward_price':a['forward_price_points'],'minimum_net_basis':a['net_basis_per100'],
            'maximum_implied_repo':-a['implied_repo_decimal']}[future['selection']]
    ranked=sorted(alternatives,key=score); fair=min(alternatives,key=lambda a:a['forward_price_points'])
    return dict(alternatives=alternatives,members=prices,selected=ranked[0]['instrument'],fair_deliverable=fair['instrument'],
        fair_price_points=fair['forward_price_points'],selection_gap=score(ranked[1])-score(ranked[0]) if len(ranked)>1 else None,
        selection_ties=[a['instrument'] for a in alternatives if score(a)==score(ranked[0])],
        fair_ties=[a['instrument'] for a in alternatives if a['forward_price_points']==fair['forward_price_points']],delta=delta)


def answer(product, resources, factor, policy, quantity=1):
    h=D(str(policy['bump'])); steps=[D(0),h,-h,h/2,-h/2]
    values=[valuation(product,resources,factor,d) for d in steps]
    mult=D(product['input']['contract']['currency_per_price_point'])*quantity
    money=[v['fair_price_points']*mult for v in values]
    size=D('.0001') if factor['unit']=='currency_per_bp' else D(1)
    frozen_id=values[0]['fair_deliverable']
    frozen=[next(a['forward_price_points'] for a in v['alternatives'] if a['instrument']==frozen_id)*mult for v in values]
    selection_switch=any(v['selected']!=values[0]['selected'] for v in values)
    fair_switch=any(v['fair_deliverable']!=frozen_id for v in values)
    ties=any(len(v['fair_ties'])>1 or len(v['selection_ties'])>1 for v in values)
    return dict(evaluations=values,base_pv=money[0],plus_pv=money[1],minus_pv=money[2],half_plus_pv=money[3],half_minus_pv=money[4],
        value=(money[1]-money[2])/(2*h)*size,half_value=(money[3]-money[4])/h*size,
        frozen_deliverable_sensitivity=(frozen[1]-frozen[2])/(2*h)*size,
        frozen_deliverable_half_sensitivity=(frozen[3]-frozen[4])/h*size,
        one_sided_plus=(money[1]-money[0])/h*size,one_sided_minus=(money[0]-money[2])/h*size,
        selection_switch=selection_switch,fair_deliverable_switch=fair_switch,ties_present=ties,
        ctd_stable=not(selection_switch or fair_switch or ties))


def request(resources, product, factor, policy, quantity=1):
    return dict(schema_version=1,inputs=resources,operation=dict(kind='calibration',input=graph(),action=dict(kind='product_risk',
        product=product,key=factor,position=position(quantity,product['input']['contract']['currency']),
        snapshot_id='S.DELIVERY',settings=policy)))


def generate():
    cases=[]
    with localcontext() as context:
        context.prec=60
        for currency,control,method,axis,quantity in [
            ('USD','stable','minimum_forward_price','discount',1),
            ('USD','switch','minimum_forward_price','discount',1),
            ('USD','switch','minimum_forward_price','discount',-3),
            ('EUR','switch','minimum_forward_price','discount',2),
            ('USD','tie','minimum_forward_price','discount',1),
            ('USD','switch','minimum_forward_price','basis_a',1),
            ('USD','stable','minimum_forward_price','basis_b',1),
            ('USD','stable','minimum_forward_price','repo_a',1),
            ('USD','switch','minimum_forward_price','repo_a',1),
            ('USD','switch','minimum_forward_price','repo_b',1),
            ('USD','criteria','maximum_implied_repo','discount',1),
            ('USD','criteria','minimum_net_basis','discount',1),
            ('USD','stable','minimum_forward_price','discount_node',1),
        ]:
            resources,product=pack(currency,control);product['input']['selection']=method
            factor=key('discount' if axis=='discount_node' else axis,product['input'],1 if axis=='discount_node' else None)
            policy=settings('0.01' if axis.startswith('basis') else '0.0002')
            expected=answer(product,resources,factor,policy,quantity)
            bound=D(str(policy['stability']['absolute']))+D(str(policy['stability']['relative']))*max(abs(expected['value']),abs(expected['half_value']))
            stable=abs(expected['value']-expected['half_value'])<=bound
            identifier='delivery.'+'.'.join([currency,control,method,axis,str(quantity)])
            cases.append(dict(id=identifier,request=request(resources,product,factor,policy,quantity),expected=expected,
                expected_error=None if stable else 'invalid_input', independent_stability_passed=stable))
    return dict(qualification='Full independent Decimal60 cash/carry/CTD controls, declared before native execution. No native financial answer or certificate.',
        source_reads={SOURCE:hashlib.sha256((ROOT/SOURCE).read_bytes()).hexdigest()},
        bounds=dict(per100=1e-8,monetary=1e-5,risk=1e-5,rate=1e-12,exact_invoice=True),cases=cases,certified=False)


if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    raw=(json.dumps(generate(),indent=2,default=serial)+'\n').encode()
    if args.write:
        if TARGET.exists(): raise ValueError('Refusing to overwrite full independent financial controls')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes()!=raw: raise ValueError('Original independent controls differ')
    print(json.dumps(dict(status='passed',controls=len(generate()['cases']),certified=False)))
