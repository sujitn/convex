#!/usr/bin/env python3
"""Independent Decimal60 Treasury Appendix B VI answers; no native imports."""
import argparse
import copy
import datetime as dt
from decimal import Decimal as D, localcontext, ROUND_HALF_UP
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'tests/fixtures/library'
SOURCES = [
    'https://www.treasurydirect.gov/files/laws-and-regulations/auction-regulations-uoc/31-cfr-part-356.pdf',
    'https://www.treasurydirect.gov/files/laws-and-regulations/uoc-noncomp-increase.pdf',
]


def answers(issue, settlement, maturity, face, price):
    """Use the published quadratic directly, independently of native inversion."""
    year = (issue.replace(year=issue.year + 1) - issue).days
    remaining = (maturity - settlement).days
    t = D(remaining) / D(year)
    long = 2 * remaining > year
    a = (t - D('.5')) / 2 if long else D(0)
    gain = face / price - 1
    rate = (-t + (t*t + 4*a*gain).sqrt()) / (2*a) if long else gain/t
    den = 1 + t*rate + a*rate*rate
    slope = t + 2*a*rate
    p100 = price / face * 100
    first = -100 * slope / den**2
    second = 100 * (2*slope*slope / den**3 - 2*a / den**2)
    discount = (1 - price/face) * 360 / D(remaining)
    return dict(
        basis_days=year, remaining_days=remaining,
        method='coupon_equivalent' if long else 'simple',
        price_original_face=str(price), price_per100=str(p100),
        auction_price_per100=str(p100.quantize(D('.000001'), rounding=ROUND_HALF_UP)),
        bank_discount_decimal=str(discount), investment_decimal=str(rate),
        investment_risk=dict(d_price_per100_d_decimal_rate=str(first),
                             d2_price_per100_d_decimal_rate2=str(second),
                             signed_price_per100_per_up_1bp=str(first/D(10000))),
        bank_discount_risk=dict(d_price_per100_d_decimal_rate=str(-D(100)*remaining/360),
                                d2_price_per100_d_decimal_rate2='0',
                                signed_price_per100_per_up_1bp=str(-D(remaining)/36000)),
    )


def build():
    seed = next(c['request'] for c in json.loads((FIXTURES/'cases.json').read_bytes())['cases']
                if c['id'] == 'zero.bill_conversions')
    cases, references, probes = [], [], []

    def add(identifier, issue, settlement, maturity, face, quote, expected_error=None):
        request = copy.deepcopy(seed)
        terms = request['operation']['input']
        terms.update(issue_date=str(issue), maturity=str(maturity), face_value=str(face),
                     redemption_value=str(face), issue_price=None)
        request['operation']['action'] = dict(kind='treasury_bill', settlement=str(settlement), quote=quote)
        cases.append(dict(id=identifier, request=request, expected_error=expected_error))
        return cases[-1]

    with localcontext() as ctx:
        ctx.prec = 60
        # Original Treasury examples plus issue/leap/half-year/seasoned/century controls.
        specs = [
            ('published.90', '1989-11-24', 0, 90, '100', '98.097500'),
            ('published.182', '1982-12-30', 0, 182, '100', '95.934567'),
            ('published.20', '1990-06-01', 0, 20, '100', '99.559444'),
            ('published.364', '1990-06-07', 0, 364, '100', '92.265000'),
            ('anniversary.before', '2019-02-28', 0, 182, '1000', '975'),
            ('anniversary.after', '2019-03-01', 0, 183, '1000', '975'),
            ('anniversary.seasoned', '2019-03-01', 200, 364, '1000', '980'),
            ('anniversary.newyear', '2020-03-01', 0, 183, '1000', '975'),
            ('century.nonleap', '2099-03-01', 0, 183, '1000', '980'),
            ('century.leap', '1999-03-01', 0, 183, '1000', '980'),
            ('precision.discount', '2026-10-13', 0, 364, '12345.67890123456789', '12001.1234567890123456789'),
            ('precision.round_up', '2026-10-13', 0, 90, '100', '99.0000005'),
        ]
        for basis_issue in ['2026-01-01', '2019-03-01']:
            for days in [1, 181, 182, 183, 184, 364, 365]:
                for name, price in [('discount', '97'), ('par', '100'), ('premium', '103')]:
                    specs.append((f'boundary.{basis_issue}.{days}.{name}', basis_issue, 0, days, '100', price))
        for name, issued, offset, tenor, f, p in specs:
            issue = dt.date.fromisoformat(issued)
            settlement, maturity = issue+dt.timedelta(days=offset), issue+dt.timedelta(days=tenor)
            face, price = D(f), D(p)
            expected = answers(issue, settlement, maturity, face, price)
            for axis in ['price', 'bank_discount', 'investment']:
                quote = (dict(kind=axis, price_original_face=p) if axis == 'price' else
                         dict(kind=axis, decimal_rate=expected[axis+'_decimal']))
                # Wire decimals retain the original 96-bit/28-scale domain. Declare
                # derived rates to 24 significant digits, separate from the ideal answer.
                if axis != 'price':
                    quote['decimal_rate'] = format(D(quote['decimal_rate']), '.24g')
                identifier = f'treasury.{name}.{axis}'
                row = add(identifier, issue, settlement, maturity, face, quote)
                # Recompute ideal answer from this exact declared wire quote.
                if axis == 'bank_discount':
                    actual_price = face * (1-D(quote['decimal_rate'])*D(expected['remaining_days'])/360)
                elif axis == 'investment':
                    rate = D(quote['decimal_rate']); t = D(expected['remaining_days'])/D(expected['basis_days'])
                    actual_price = face/(1+t*rate) if expected['method']=='simple' else face/((1+(t-D('.5'))*rate)*(1+rate/2))
                else:
                    actual_price = price
                references.append(dict(id=identifier, expected=answers(issue, settlement, maturity, face, actual_price)))
                if axis == 'investment':
                    ids = {}
                    for hname, h in [('h',D('.0001')), ('h2',D('.00005'))]:
                        for sign, direction in [('up',1), ('down',-1)]:
                            q = dict(kind='investment',decimal_rate=str(D(quote['decimal_rate'])+direction*h))
                            probe_id = identifier+'.'+hname+'.'+sign
                            add(probe_id, issue, settlement, maturity, face, q)
                            t=D(expected['remaining_days'])/D(expected['basis_days']); rate=D(q['decimal_rate'])
                            pr=face/(1+t*rate) if expected['method']=='simple' else face/((1+(t-D('.5'))*rate)*(1+rate/2))
                            references.append(dict(id=probe_id,expected=answers(issue,settlement,maturity,face,pr)))
                            ids[hname+'.'+sign]=probe_id
                    probes.append(dict(id=identifier, inputs=ids))

        positive=copy.deepcopy(cases[0])
        def bad(identifier, mutate, code='invalid_input'):
            c=copy.deepcopy(positive); c['id']='treasury.invalid.'+identifier; c['expected_error']=code
            mutate(c['request']); cases.append(c)
        def action(r): return r['operation']['action']
        def terms(r): return r['operation']['input']
        bad('zero_price',lambda r: action(r)['quote'].update(price_original_face='0'))
        bad('negative_price',lambda r: action(r)['quote'].update(price_original_face='-1'))
        bad('numeric_price',lambda r: action(r)['quote'].update(price_original_face=98.0975))
        bad('unknown_quote_field',lambda r: action(r)['quote'].update(cached_yield='0.04'))
        bad('unknown_quote',lambda r: action(r)['quote'].update(kind='street'))
        bad('before_issue',lambda r: action(r).update(settlement='1989-11-23'))
        bad('at_maturity',lambda r: action(r).update(settlement='1990-02-22'))
        bad('after_maturity',lambda r: action(r).update(settlement='1990-02-23'))
        bad('non_usd',lambda r: terms(r).update(currency='EUR'), 'unsupported')
        bad('nonface_redemption',lambda r: terms(r).update(redemption_value='105'), 'unsupported')
        bad('over_one_year',lambda r: terms(r).update(maturity='1991-01-01'), 'unsupported')
        bad('feb29_issue',lambda r: (terms(r).update(issue_date='2020-02-29',maturity='2020-06-01'),action(r).update(settlement='2020-02-29')), 'unsupported')
        bad('missing_calendar',lambda r: r['inputs'].update(calendars=[]),'missing_input')
        bad('negative_face',lambda r: terms(r).update(face_value='-100'))
        bad('zero_bank_price',lambda r: action(r).update(quote=dict(kind='bank_discount',decimal_rate='4')))
        bad('investment_pole',lambda r: action(r).update(quote=dict(kind='investment',decimal_rate='-5')))
        bad('long_investment_pole',lambda r: (terms(r).update(maturity='1990-11-23'),action(r).update(quote=dict(kind='investment',decimal_rate='-2'))))
        bad('both_long_factors_negative',lambda r: (terms(r).update(maturity='1990-11-23'),action(r).update(quote=dict(kind='investment',decimal_rate='-5'))))
        bad('cash_above_bound',lambda r: terms(r).update(face_value='1000000000000001',redemption_value='1000000000000001'),'resource_limit')
        bad('cash_below_bound',lambda r: terms(r).update(face_value='0.0000000000000001',redemption_value='0.0000000000000001'),'resource_limit')
        bad('tiny_ratio',lambda r: action(r)['quote'].update(price_original_face='0.00001'),'resource_limit')
        bad('huge_ratio',lambda r: action(r)['quote'].update(price_original_face='100000001'),'resource_limit')
    return {
        'treasury-bill.json': dict(certified=False, cases=cases),
        'treasury-bill-reference.json': dict(certified=False, precision=60, sources=SOURCES,
            qualification='Independently declared published-equation and synthetic controls; no human convention/model approval.',
            absolute_tolerance='1e-18',relative_tolerance='1e-21',
            finite_difference_first_relative='1e-7',finite_difference_second_relative='1e-7',
            cases=references,convergence=probes),
    }


def admitted():
    """Preserve original financial answers; repair only new transport admission."""
    original=build()
    corpus=copy.deepcopy(original['treasury-bill.json'])
    corpus['schema_version']=1
    for c in corpus['cases']:
        q=c['request']['operation']['action']['quote']
        if 'decimal_rate' in q and D(q['decimal_rate'])==0:
            q['decimal_rate']='0'
    reference=copy.deepcopy(original['treasury-bill-reference.json'])
    reference['transport_admission']='Original equation answers unchanged; zero decimal plain encoding and corpus schema_version only.'
    return {'treasury-bill-admitted.json':corpus,
            'treasury-bill-reference-admitted.json':reference}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command',choices=['write','write-admitted','check'])
    args=parser.parse_args()
    results={}
    declared=admitted() if args.command=='write-admitted' else build()
    if args.command=='check': declared.update(admitted())
    for name,data in declared.items():
        raw=(json.dumps(data,indent=2)+'\n').encode(); path=FIXTURES/name
        if args.command.startswith('write'):
            if path.exists(): raise ValueError('Refusing to replace a declared control: '+name)
            path.write_bytes(raw)
        elif path.read_bytes()!=raw: raise ValueError('Independent control bytes differ: '+name)
        results[name]=dict(cases=len(data['cases']),sha256=hashlib.sha256(raw).hexdigest())
    print(json.dumps(dict(status='passed',controls=results,certified=False)))


if __name__=='__main__': main()
