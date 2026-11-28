"""The desk demo's synthetic seed, generated for one launch (stdlib only).

Everything here is fictional and labelled demo: the issuers, the bonds, the books, the
curves and the fixings. It writes, into a work directory:

  seed.json       the `convex-core-inputs` file: reference data (bonds and their masters,
                  pricing rules, the Treasury benchmark set, the books), curve recipes,
                  SOFR fixings through the last business day and the pricing configuration
  selection.json  the `convex-config compile` selection: the books, quote sources, measures
                  and client tiers, settling on the launch's settlement date
  fixtures.json   one activation fixture: a quote for every bond and the value each must
  events.jsonl    give exactly; and its events as the replay file
  feed.json       the feed's instruments, starting levels and sensitivities
  desk.json       what the launcher sets up once the server runs: the desk's marks, each
                  book's limits, and the bonds and books the simulators trade

The activation fixture asserts each bond's quote back (a clean price, a yield to worst, a
discount margin, a sinker's price times its factor), except a bond on a spread to a
Treasury, whose yield is the Treasury's street yield plus the spread. That yield comes from
`street_clean_price` below, an independent street-convention price for a Treasury paying
unadjusted on its regular semiannual schedule, accruing ACT/ACT ICMA.
"""
from __future__ import annotations

import copy
import json
from dataclasses import dataclass, field
from datetime import date, datetime, timedelta, timezone
from decimal import ROUND_HALF_EVEN, Decimal, getcontext
from pathlib import Path

getcontext().prec = 50

ROOT = Path(__file__).resolve().parents[2]
CONFIG_ID = 'DESK-USD'
FEED_SOURCE = 'desk-feed'
LEVEL_SOURCE = 'desk'
SET_ID = 'UST-OTR'
INDEX_ID = 'USD.SOFR'  # the projection recipe's index, which the rate index names
FIXING_SOURCE = 'demo-administrator'
SOFR = Decimal('0.0430')
CALENDAR = {'id': 'cal-sifma', 'version': 1}
EFFECTIVE = {'from': '2020-01-01T00:00:00Z', 'until': None}
PROVENANCE = {'source': 'desk-demo', 'evidence_reference': 'synthetic'}
TIERS = [('1', '0.75'), ('2', '1'), ('3', '1.5')]
MEASURES = [
    'clean_price_mid', 'dirty_price_mid', 'accrued_interest', 'ytm_mid', 'ytw_mid',
    'years_to_worst_mid', 'modified_duration_mid', 'convexity_mid', 'dv01_mid', 'cs01_mid',
    'z_spread_mid', 'g_spread_mid', 'discount_margin_mid', 'factor',
    'krd_2y_mid', 'krd_5y_mid', 'krd_10y_mid', 'krd_30y_mid',
]


# ---------------------------------------------------------------------------------------
# Dates
# ---------------------------------------------------------------------------------------

def add_months(day: date, months: int) -> date:
    """`day` moved by whole months; the day of month is at most 28 here, so it always exists."""
    index = day.year * 12 + day.month - 1 + months
    return date(index // 12, index % 12 + 1, day.day)


def weekday_on_or_after(day: date) -> date:
    while day.weekday() >= 5:
        day += timedelta(days=1)
    return day


def stamp(at: datetime) -> str:
    return at.strftime('%Y-%m-%dT%H:%M:%S.') + f'{at.microsecond // 1000:03d}Z'


# ---------------------------------------------------------------------------------------
# The independent Treasury reference
# ---------------------------------------------------------------------------------------

def street_clean_price(coupon: Decimal, issue: date, maturity: date, settlement: date, ytm: Decimal) -> Decimal:
    """A Treasury's clean price per 100 at `ytm`, compounded semiannually, by the street
    convention: paid unadjusted on its regular semiannual schedule from `issue`, accruing
    ACT/ACT ICMA, each flow discounted over its whole periods plus the fraction of the
    current period still to run."""
    months = 6
    coupons = []
    day = add_months(issue, months)
    while day <= maturity:
        coupons.append(day)
        day = add_months(day, months)
    assert coupons and coupons[-1] == maturity, 'a regular schedule'
    upcoming = [day for day in coupons if day > settlement]
    following = upcoming[0]
    previous = add_months(following, -months)
    period = Decimal((following - previous).days)
    to_run = Decimal((following - settlement).days) / period
    per_period = coupon * 100 / 2
    growth = 1 + ytm / 2
    dirty = Decimal(0)
    for index, _ in enumerate(upcoming):
        flow = per_period + (100 if index == len(upcoming) - 1 else 0)
        dirty += flow / growth ** (Decimal(index) + to_run)
    accrued = per_period * Decimal((settlement - previous).days) / period
    return dirty - accrued


def modified_duration(coupon: Decimal, issue: date, maturity: date, settlement: date, ytm: Decimal) -> float:
    """By a one basis point central difference of the street price: for the feed's moves only."""
    bump = Decimal('0.0001')
    up = street_clean_price(coupon, issue, maturity, settlement, ytm + bump)
    down = street_clean_price(coupon, issue, maturity, settlement, ytm - bump)
    mid = street_clean_price(coupon, issue, maturity, settlement, ytm)
    return float((down - up) / (2 * bump * mid))


def percent(rate: str) -> str:
    """A decimal rate as a coupon is written: 0.04625 as 4.625."""
    return f'{(Decimal(rate) * 100).normalize():f}'


def cusip(base: str) -> str:
    """`base` (8 characters) with its check digit."""
    total = 0
    for position, char in enumerate(base):
        value = int(char) if char.isdigit() else ord(char) - ord('A') + 10
        if position % 2 == 1:
            value *= 2
        total += value // 10 + value % 10
    return base + str((10 - total % 10) % 10)


# ---------------------------------------------------------------------------------------
# Terms
# ---------------------------------------------------------------------------------------

SETTINGS = {
    'convention-ust': {'day_count': 'ActActIcma', 'business_day_convention': 'Unadjusted',
                       'end_of_month': False, 'settlement_days': 1, 'ex_coupon_days': 0},
    'convention-corporate': {'day_count': 'Thirty360US', 'business_day_convention': 'Following',
                             'end_of_month': False, 'settlement_days': 2, 'ex_coupon_days': 0},
    'convention-sinker': {'day_count': 'Thirty360US', 'business_day_convention': 'Unadjusted',
                          'end_of_month': False, 'settlement_days': 2, 'ex_coupon_days': 0},
    'convention-frn': {'day_count': 'Act360', 'business_day_convention': 'Following',
                       'end_of_month': False, 'settlement_days': 2, 'ex_coupon_days': 0},
}
DAY_COUNT_NAMES = {'ActActIcma': 'ACT/ACT ICMA', 'Thirty360US': '30/360 US', 'Act360': 'ACT/360'}

ISSUERS = {
    'US-TREASURY': ('United States Treasury (synthetic)', 'Sovereign', 'Government'),
    'HARBOR-UTIL': ('Harbor Point Utilities (fictional)', 'CorporateIG', 'Utilities'),
    'GRANITE-ENERGY': ('Granite Ridge Energy (fictional)', 'CorporateIG', 'Energy'),
    'BLUEWATER-TEL': ('Bluewater Telecom (fictional)', 'CorporateIG', 'Telecommunications'),
    'SUMMIT-RAIL': ('Summit Rail (fictional)', 'CorporateIG', 'Transportation'),
    'CEDAR-HEALTH': ('Cedar Health (fictional)', 'CorporateHY', 'Healthcare'),
    'IRONWOOD-STEEL': ('Ironwood Steel (fictional)', 'CorporateHY', 'Materials'),
    'MERIDIAN-BANK': ('Meridian Bank (fictional)', 'CorporateIG', 'Financials'),
}


def bond(instrument_id: str, issuer_id: str, description: str, *, issue: date, maturity: date | None,
         coupon: Decimal | None, frequency: int, convention: str, bond_type: str,
         cusip_code: str | None = None, seniority: str = 'Senior', **terms) -> dict:
    name, issuer_type, sector = ISSUERS[issuer_id]
    settings = SETTINGS[convention]
    contractual = {
        'effective': EFFECTIVE,
        'dated_date': issue.isoformat(),
        'penultimate_coupon_date': None,
        'redemption_value': '100',
        'issuer': {'entity_id': issuer_id, 'version': 1},
        'calendar': CALENDAR,
        'calendar_code': 'sifma',
        'convention': {'id': convention, 'version': 1},
        'settings': settings,
        'provenance': PROVENANCE,
        'stub': 'none',
        'payment_lag': terms.pop('payment_lag', 0),
    }
    contractual.update(terms.pop('contractual', {}))
    record = {
        'instrument_id': instrument_id, 'isin': None, 'cusip': cusip_code, 'sedol': None, 'bbgid': None,
        'description': description, 'currency': 'USD', 'issue_date': issue.isoformat(),
        'coupon_rate': None if coupon is None else str(coupon), 'frequency': frequency,
        'day_count': DAY_COUNT_NAMES[settings['day_count']], 'face_value': '100', 'bond_type': bond_type,
        'issuer_type': issuer_type, 'issuer_id': issuer_id, 'issuer_name': name, 'seniority': seniority,
        'is_callable': False, 'call_schedule': [], 'is_putable': False, 'is_sinkable': False,
        'floating_terms': None, 'inflation_index': None, 'inflation_base_index': None,
        'has_deflation_floor': False, 'country_of_risk': 'US', 'sector': sector,
        'amount_outstanding': '500000000', 'first_coupon_date': None, 'last_updated': 0,
        'source': 'desk-demo', 'contractual_terms': contractual,
    }
    if maturity is not None:
        record['maturity_date'] = maturity.isoformat()
    record.update(terms)
    return record


def master(kind: str, definition: dict) -> dict:
    return {'expected_version': None, 'definition': {'kind': kind, 'definition': definition}}


def rule(instrument_id: str, method: dict, *, benchmark_set: str | None = None, width: dict | None = None,
         tenor_basis: str | None = None) -> dict:
    definition = {'instrument_id': instrument_id, 'method': method, 'effective': EFFECTIVE, 'provenance': PROVENANCE}
    if benchmark_set:
        definition['benchmark_set_id'] = benchmark_set
    if width:
        definition['width'] = width
    if tenor_basis:
        definition['tenor_basis'] = tenor_basis
    return master('pricing_rule', definition)


def bp_width(small: str, medium: str, large: str) -> dict:
    return {'unit': 'basis_points', 'buckets': [
        {'max_face': '1000000', 'width': small}, {'max_face': '10000000', 'width': medium}, {'width': large}]}


# ---------------------------------------------------------------------------------------
# The universe
# ---------------------------------------------------------------------------------------

@dataclass
class Quote:
    """A bond's opening quote, where it is read from, and the value activation expects."""
    instrument_id: str
    source: str
    field: str
    value: str
    unit: str
    expected_measure: str
    expected_value: Decimal


@dataclass
class Universe:
    bonds: list = field(default_factory=list)
    rules: list = field(default_factory=list)
    quotes: list = field(default_factory=list)
    feed_prices: list = field(default_factory=list)
    feed_levels: list = field(default_factory=list)


TREASURIES = [  # tenor, years, coupon, opening street yield
    ('2Y', 2, '0.03875', '0.0395'),
    ('3Y', 3, '0.03875', '0.0398'),
    ('5Y', 5, '0.0400', '0.0408'),
    ('7Y', 7, '0.04125', '0.0418'),
    ('10Y', 10, '0.0425', '0.0430'),
    ('20Y', 20, '0.04625', '0.0464'),
    ('30Y', 30, '0.04625', '0.0468'),
]


def build(settlement: date) -> tuple[Universe, dict]:
    universe = Universe()
    # The current Treasuries were issued on the latest 15th on or before settlement, so each
    # pays on the 15th, regularly, and settlement is never a coupon date.
    anchor = settlement.replace(day=15) if settlement.day >= 15 else add_months(settlement.replace(day=15), -1)
    bench_yield: dict[str, Decimal] = {}
    securities = []
    for index, (tenor, years, coupon, opening) in enumerate(TREASURIES):
        maturity = add_months(anchor, 12 * years)
        coupon_rate, ytm = Decimal(coupon), Decimal(opening)
        instrument_id = f'UST-{tenor}'
        # Issue letters skip I and O, which CUSIPs do not use.
        code = cusip(f'91282C{"LMNPQRS"[index]}{index}')
        universe.bonds.append(bond(
            instrument_id, 'US-TREASURY', f'UST {percent(coupon)}% {maturity:%b-%y}',
            issue=anchor, maturity=maturity, coupon=coupon_rate, frequency=2, convention='convention-ust',
            bond_type='FixedBullet', cusip_code=code))
        price = street_clean_price(coupon_rate, anchor, maturity, settlement, ytm).quantize(Decimal('1e-12'), ROUND_HALF_EVEN)
        universe.rules.append(rule(instrument_id, {'kind': 'price'}, width={'unit': 'thirty_seconds', 'buckets': [
            {'max_face': '10000000', 'width': '0.5'}, {'width': '1'}]}))
        universe.quotes.append(Quote(instrument_id, FEED_SOURCE, 'mid_price', str(price), 'price_per100',
                                     'clean_price_mid', price))
        universe.feed_prices.append({'instrument_id': instrument_id, 'price': str(price),
                                     'modified_duration': round(modified_duration(coupon_rate, anchor, maturity, settlement, ytm), 6),
                                     'credit_beta': 0.0})
        bench_yield[tenor] = ytm
        securities.append({'tenor': tenor, 'instrument_id': instrument_id, 'version': 1})
    benchmark_set = master('benchmark_set', {'benchmark_set_id': SET_ID, 'currency': 'USD',
                                             'securities': securities, 'effective': EFFECTIVE, 'provenance': PROVENANCE})

    roll = 10 if settlement.day != 10 else 11  # no corporate coupon or sink falls on settlement

    def corporate_issue(years_ago: int) -> date:
        return add_months(settlement.replace(day=roll), -12 * years_ago)

    # Investment grade bullets, each on a spread to the Treasury of its tenor.
    for instrument_id, issuer, coupon, years_ago, term, tenor, spread in [
        ('IG-HPU-30', 'HARBOR-UTIL', '0.0465', 1, 5, '5Y', '92'),
        ('IG-HPU-35', 'HARBOR-UTIL', '0.0510', 1, 10, '10Y', '118'),
        ('IG-GRE-29', 'GRANITE-ENERGY', '0.0450', 1, 4, '3Y', '105'),
        ('IG-GRE-34', 'GRANITE-ENERGY', '0.0525', 2, 10, '7Y', '135'),
        ('IG-BWT-31', 'BLUEWATER-TEL', '0.0475', 1, 6, '5Y', '128'),
        ('IG-BWT-55', 'BLUEWATER-TEL', '0.0560', 1, 30, '30Y', '162'),
        ('IG-SRL-33', 'SUMMIT-RAIL', '0.0490', 1, 8, '7Y', '84'),
        ('IG-SRL-45', 'SUMMIT-RAIL', '0.0540', 1, 20, '20Y', '110'),
        ('IG-MER-29', 'MERIDIAN-BANK', '0.0440', 0, 3, '2Y', '72'),
        ('IG-MER-36', 'MERIDIAN-BANK', '0.0515', 0, 10, '10Y', '138'),
    ]:
        issue = corporate_issue(years_ago)
        maturity = add_months(issue, 12 * term)
        universe.bonds.append(bond(instrument_id, issuer, f'{ISSUERS[issuer][0].split(" (")[0]} {percent(coupon)}% {maturity:%b-%y}',
                                   issue=issue, maturity=maturity, coupon=Decimal(coupon), frequency=2,
                                   convention='convention-corporate', bond_type='FixedBullet'))
        universe.rules.append(rule(instrument_id, {'kind': 'benchmark_spread', 'tenor': tenor},
                                   benchmark_set=SET_ID, width=bp_width('2', '4', '8')))
        universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_spread', spread, 'spread_basis_points',
                                     'ytm_mid', bench_yield[tenor] + Decimal(spread) / 10000))
        universe.feed_levels.append({'instrument_id': instrument_id, 'field': 'mid_spread', 'credit_beta': 1.0})

    # High yield callables: two quoted by price, two by yield to worst; mid only, as a
    # width applies to bullets.
    for instrument_id, issuer, coupon, term, first_call, method, level in [
        ('HY-CDH-31', 'CEDAR-HEALTH', '0.0675', 7, 2, 'price', '101.25'),
        ('HY-CDH-33', 'CEDAR-HEALTH', '0.0725', 8, 3, 'yield', '0.0705'),
        ('HY-IRS-30', 'IRONWOOD-STEEL', '0.0800', 6, 1, 'price', '102.5'),
        ('HY-IRS-32', 'IRONWOOD-STEEL', '0.0850', 8, 2, 'yield', '0.0790'),
    ]:
        issue = corporate_issue(2)
        maturity = add_months(issue, 12 * term)
        calls, premium = [], Decimal(coupon) * 50
        for year in range(first_call, term):
            price = max(Decimal(100), Decimal(100) + premium * (term - 1 - year) / max(1, term - 1 - first_call))
            calls.append({'call_date': add_months(issue, 12 * year).isoformat(), 'call_price': str(price.quantize(Decimal('0.001'))),
                          'is_make_whole': False})
        universe.bonds.append(bond(instrument_id, issuer, f'{ISSUERS[issuer][0].split(" (")[0]} {percent(coupon)}% {maturity:%b-%y} callable',
                                   issue=issue, maturity=maturity, coupon=Decimal(coupon), frequency=2,
                                   convention='convention-corporate', bond_type='FixedCallable', is_callable=True,
                                   call_schedule=calls, contractual={'call_terms': {'style': 'bermudan', 'notice_days': 30}}))
        if method == 'price':
            universe.rules.append(rule(instrument_id, {'kind': 'price'}))
            universe.quotes.append(Quote(instrument_id, FEED_SOURCE, 'mid_price', level, 'price_per100', 'clean_price_mid', Decimal(level)))
            universe.feed_prices.append({'instrument_id': instrument_id, 'price': level, 'modified_duration': 3.0, 'credit_beta': 2.5})
        else:
            universe.rules.append(rule(instrument_id, {'kind': 'yield'}))
            universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_yield', level, 'yield_decimal', 'ytw_mid', Decimal(level)))
            universe.feed_levels.append({'instrument_id': instrument_id, 'field': 'mid_yield', 'credit_beta': 2.5})

    # Floating rate notes on SOFR, quoted by discount margin.
    for instrument_id, issuer, quoted_margin, term, margin in [
        ('FRN-MER-29', 'MERIDIAN-BANK', '65', 3, '58'),
        ('FRN-HPU-30', 'HARBOR-UTIL', '80', 4, '74'),
    ]:
        issue = corporate_issue(1)
        maturity = add_months(issue, 12 * term)
        universe.bonds.append(bond(instrument_id, issuer, f'{ISSUERS[issuer][0].split(" (")[0]} SOFR+{quoted_margin} {maturity:%b-%y}',
                                   issue=issue, maturity=maturity, coupon=None, frequency=4, convention='convention-frn',
                                   bond_type='FloatingRate', payment_lag=2,
                                   floating_terms={'index': 'Sofr', 'spread': quoted_margin, 'reset_frequency': 4, 'cap': None, 'floor': None},
                                   contractual={'floating_mechanics': {'index': {'id': 'usd-sofr', 'version': 1}, 'overrides': {}}}))
        universe.rules.append(rule(instrument_id, {'kind': 'discount_margin'}))
        universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_spread', margin, 'spread_basis_points', 'discount_margin_mid', Decimal(margin)))
        universe.feed_levels.append({'instrument_id': instrument_id, 'field': 'mid_spread', 'credit_beta': 0.5})

    # Sinking-fund bonds, quoted by price per 100 current face.
    for instrument_id, issuer, coupon, term, level in [
        ('SNK-SRL-34', 'SUMMIT-RAIL', '0.0560', 12, '99.5'),
        ('SNK-GRE-37', 'GRANITE-ENERGY', '0.0600', 14, '101.0'),
    ]:
        issue = corporate_issue(4)
        maturity = add_months(issue, 12 * term)
        # Eight per cent of the original face a year from the third, the rest at maturity.
        sinks = [{'date': add_months(issue, 12 * year).isoformat(), 'amount': '8', 'price': '100'} for year in range(3, term - 2)]
        sunk = sum(Decimal(sink['amount']) for sink in sinks if date.fromisoformat(sink['date']) < settlement)
        factor = 1 - sunk / 100
        universe.bonds.append(bond(instrument_id, issuer, f'{ISSUERS[issuer][0].split(" (")[0]} {percent(coupon)}% {maturity:%b-%y} sinker',
                                   issue=issue, maturity=maturity, coupon=Decimal(coupon), frequency=2,
                                   convention='convention-sinker', bond_type='Amortizing', is_sinkable=True,
                                   contractual={'sinking_schedule': sinks}))
        universe.rules.append(rule(instrument_id, {'kind': 'price'}))
        universe.quotes.append(Quote(instrument_id, FEED_SOURCE, 'mid_price', level, 'price_per100', 'clean_price_mid', Decimal(level) * factor))
        universe.feed_prices.append({'instrument_id': instrument_id, 'price': level, 'modified_duration': 5.0, 'credit_beta': 1.0})

    # Callable perpetuals, quoted by yield to worst.
    for instrument_id, issuer, coupon, first_call_years, level in [
        ('PERP-MER', 'MERIDIAN-BANK', '0.0650', 4, '0.0610'),
        ('PERP-BWT', 'BLUEWATER-TEL', '0.0700', 3, '0.0675'),
    ]:
        issue = corporate_issue(1)
        first_call = add_months(issue, 12 * first_call_years)
        universe.bonds.append(bond(instrument_id, issuer, f'{ISSUERS[issuer][0].split(" (")[0]} {percent(coupon)}% perpetual',
                                   issue=issue, maturity=None, coupon=Decimal(coupon), frequency=2,
                                   convention='convention-corporate', bond_type='Perpetual', is_callable=True,
                                   seniority='Subordinated',
                                   contractual={'perpetual': {'first_call_date': first_call.isoformat(), 'call_interval_months': 6,
                                                              'call_price': '100', 'notice_days': 30}}))
        universe.rules.append(rule(instrument_id, {'kind': 'yield'}))
        universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_yield', level, 'yield_decimal', 'ytw_mid', Decimal(level)))
        universe.feed_levels.append({'instrument_id': instrument_id, 'field': 'mid_yield', 'credit_beta': 2.0})

    return universe, benchmark_set


BOOKS = {
    'BOOK-IG': ('Investment grade credit', [
        ('IG-HPU-30', '5000000', 'A-'), ('IG-HPU-35', '3000000', 'A-'), ('IG-GRE-29', '4000000', 'BBB+'),
        ('IG-GRE-34', '2000000', 'BBB+'), ('IG-BWT-31', '6000000', 'BBB'), ('IG-BWT-55', '2000000', 'BBB'),
        ('IG-SRL-33', '3000000', 'A'), ('IG-SRL-45', '2000000', 'A'), ('IG-MER-29', '5000000', 'A+'),
        ('IG-MER-36', '3000000', 'A+'), ('UST-5Y', '-8000000', 'AA+'), ('UST-10Y', '-5000000', 'AA+'),
    ]),
    'BOOK-HY': ('High yield, sinkers and perpetuals', [
        ('HY-CDH-31', '4000000', 'BB-'), ('HY-CDH-33', '3000000', 'BB-'), ('HY-IRS-30', '2000000', 'B+'),
        ('HY-IRS-32', '2000000', 'B+'), ('SNK-SRL-34', '3000000', 'A'), ('SNK-GRE-37', '2000000', 'BBB+'),
        ('PERP-MER', '2000000', 'BBB-'), ('PERP-BWT', '1000000', 'BB+'),
    ]),
    'BOOK-FLOW': ('Rates and floaters', [
        ('UST-2Y', '10000000', 'AA+'), ('UST-3Y', '5000000', 'AA+'), ('UST-7Y', '-3000000', 'AA+'),
        ('UST-20Y', '2000000', 'AA+'), ('UST-30Y', '-1000000', 'AA+'), ('FRN-MER-29', '8000000', 'A+'),
        ('FRN-HPU-30', '5000000', 'A-'),
    ]),
}

LIMITS = {
    'BOOK-IG': [
        {'limit_id': 'MV', 'measure': {'kind': 'market_value'}, 'lower': '-10000000', 'upper': '40000000', 'warning_fraction': '0.85'},
        {'limit_id': 'DV01', 'measure': {'kind': 'dv01'}, 'lower': '-15000', 'upper': '15000', 'warning_fraction': '0.8'},
        {'limit_id': 'CS01', 'measure': {'kind': 'cs01'}, 'lower': '-25000', 'upper': '25000', 'warning_fraction': '0.8'},
        {'limit_id': 'KRD-10Y', 'measure': {'kind': 'key_rate_dv01', 'tenor': '10Y'}, 'lower': '-6000', 'upper': '6000', 'warning_fraction': '0.8'},
        {'limit_id': 'ISSUER', 'measure': {'kind': 'group_share', 'by': 'issuer', 'group': 'each'}, 'upper': '0.35', 'warning_fraction': '0.8'},
        {'limit_id': 'FACE', 'measure': {'kind': 'face', 'instrument': 'each'}, 'lower': '-10000000', 'upper': '8000000'},
    ],
    'BOOK-HY': [
        {'limit_id': 'MV', 'measure': {'kind': 'market_value'}, 'upper': '20000000', 'warning_fraction': '0.85'},
        {'limit_id': 'GROSS', 'measure': {'kind': 'gross_market_value'}, 'upper': '22000000', 'warning_fraction': '0.85'},
        {'limit_id': 'ISSUER', 'measure': {'kind': 'group_share', 'by': 'issuer', 'group': 'each'}, 'upper': '0.40', 'warning_fraction': '0.8'},
        {'limit_id': 'RATING-B', 'measure': {'kind': 'group_market_value', 'by': 'rating', 'group': {'one': 'B+'}}, 'upper': '5000000', 'warning_fraction': '0.8'},
        {'limit_id': 'FACE', 'measure': {'kind': 'face', 'instrument': 'each'}, 'lower': '-5000000', 'upper': '5000000'},
    ],
    'BOOK-FLOW': [
        {'limit_id': 'MV', 'measure': {'kind': 'market_value'}, 'upper': '35000000', 'warning_fraction': '0.85'},
        {'limit_id': 'DV01', 'measure': {'kind': 'dv01'}, 'lower': '-8000', 'upper': '8000', 'warning_fraction': '0.8'},
        {'limit_id': 'FACE', 'measure': {'kind': 'face', 'instrument': 'each'}, 'lower': '-10000000', 'upper': '12000000'},
    ],
}


def conventions() -> list:
    return [master('convention', {'convention_id': convention_id, 'settings': settings, 'effective': EFFECTIVE,
                                  'provenance': PROVENANCE}) for convention_id, settings in SETTINGS.items()]


def generate(work: Path, now: datetime | None = None) -> dict:
    """Write the seed for a launch at `now` into `work`; return the launch's facts."""
    now = (now or datetime.now(timezone.utc)).replace(microsecond=0)
    recorded = stamp(now - timedelta(seconds=30))
    settlement = weekday_on_or_after(now.date())
    universe, benchmark_set = build(settlement)
    held = {instrument for _, positions in BOOKS.values() for instrument, _, _ in positions}
    missing = {b['instrument_id'] for b in universe.bonds} - held - {s['instrument_id'] for s in benchmark_set['definition']['definition']['securities']}
    assert not missing, f'every bond is held or a benchmark: {missing}'

    entities = [master('entity', {'entity_id': issuer_id, 'legal_name': name, 'parent': None, 'country': 'US',
                                  'effective': EFFECTIVE, 'provenance': PROVENANCE}) for issuer_id, (name, _, _) in ISSUERS.items()]
    calendar = master('calendar', {'calendar_id': CALENDAR['id'], 'code': 'sifma', 'rule_version': 'convex-calendar-v1',
                                   'coverage_from': '1990-01-01', 'coverage_until': '2100-12-31',
                                   'effective': EFFECTIVE, 'provenance': PROVENANCE})
    rate_index = master('rate_index', {
        'rate_index_id': 'usd-sofr', 'index_id': INDEX_ID, 'currency': 'USD', 'kind': 'overnight',
        'fixing_source': FIXING_SOURCE, 'calendar': CALENDAR, 'calendar_code': 'sifma',
        'publication_lag_business_days': 1, 'publication_utc_seconds': 13 * 3600,
        'conventions': {'lookback_business_days': 2, 'observation_shift': False, 'lockout_business_days': 0},
        'effective': EFFECTIVE, 'provenance': PROVENANCE})
    books = [master('book', {
        'book_id': book_id, 'owning_system': 'desk-bms', 'name': name, 'currency': 'USD', 'description': 'Synthetic demo book',
        'effective': {'from': recorded, 'until': None},
        'positions': [{'instrument_id': instrument, 'quantity': quantity, 'unit': 'face', 'reference_version': None,
                       'source_identifier': None, 'sector': None, 'rating': rating} for instrument, quantity, rating in positions],
        'provenance': PROVENANCE}) for book_id, (name, positions) in BOOKS.items()]

    commands = [
        ('demo-masters', [], entities + [calendar, rate_index] + conventions()),
        ('demo-bonds', [{'kind': 'create', 'bond': b} for b in universe.bonds], []),
        ('demo-rules', [], universe.rules + [benchmark_set]),
        ('demo-books', [], books),
    ]
    references = [{'recorded_at': recorded, 'command': {'idempotency_key': key, 'changes': changes, 'master_changes': masters}}
                  for key, changes, masters in commands]

    showcase = json.loads((ROOT / 'deploy/showcase/seed.json').read_text())
    curves = []
    for curve in showcase['curves']:
        if curve['curve_id'] in ('D.OIS', 'P.SOFR'):
            fresh = copy.deepcopy(curve)
            fresh['source_time'] = fresh['recorded_at'] = recorded
            curves.append(fresh)
    assert {c['recipe']['role'].get('index_id') for c in curves} >= {INDEX_ID}, 'the projection recipe names the index'

    recorded_ms = int((now - timedelta(seconds=30)).timestamp() * 1000)
    pricing = {
        'config_id': CONFIG_ID, 'description': 'Desk demo: synthetic USD bonds',
        'applies_to': {'currency': 'USD'}, 'pricing_spec': {'MarketQuote': {'side': 'Mid'}}, 'bid_ask_spread': None,
        'analytics_curves': {'discount_curve': 'D.OIS', 'government_curve': None, 'swap_curve': 'P.SOFR',
                             'frn_reference_curve': 'P.SOFR'},
        'priority': None, 'active': True, 'version': 1, 'created_at': recorded_ms, 'updated_at': recorded_ms,
    }

    # SOFR fixings for each weekday before settlement, from a quarter's coupon period back.
    fixings = []
    day = settlement - timedelta(days=200)
    while day < settlement:
        if day.weekday() < 5:
            fixings.append({'schema_version': 1, 'key': {'index_id': INDEX_ID, 'observation_date': day.isoformat(), 'source': FIXING_SOURCE},
                            'version': 1, 'rate': {'unit': 'decimal_rate', 'value': str(SOFR)}, 'published_at': recorded,
                            'recorded_at': recorded, 'is_preliminary': False, 'entitled': True, 'evidence_reference': 'synthetic'})
        day += timedelta(days=1)

    seed = {'schema_version': 1, 'references': references, 'curves': curves, 'fixings': fixings, 'pricing_configs': [pricing]}
    cutoff = stamp(now)
    selection = {
        'schema_version': 1, 'pricing_config': CONFIG_ID, 'books': list(BOOKS), 'source': FEED_SOURCE, 'fields': ['mid_price'],
        'quote_max_age_ms': 600000, 'max_source_skew_ms': 600000, 'as_of': cutoff, 'known_at': cutoff, 'mode': 'live',
        'settlement_date': settlement.isoformat(), 'measures': MEASURES,
        'numerical_settings': {'policy_id': 'convex-bond-router-v1', 'bump_bps': 1.0}, 'max_concurrent_calcs': 4,
        'levels': {'source': LEVEL_SOURCE, 'max_age_ms': 86400000},
        'client_tiers': [{'client_tier': tier, 'width_factor': factor} for tier, factor in TIERS],
    }
    now_ms = int(now.timestamp() * 1000)
    events = [{'schema_version': 1, 'key': {'instrument_id': q.instrument_id, 'source': q.source}, 'session': 'demo-fixture',
               'sequence': 1, 'source_time_ms': now_ms, 'receive_time_ms': now_ms, 'quality': 'live', 'entitled': True,
               'entitlement_tags': [], 'evidence_reference': None,
               'event': {'kind': 'image', 'fields': [{'field': q.field, 'update': {'action': 'set', 'value': {'value': q.value, 'unit': q.unit}}}]}}
              for q in universe.quotes]
    fixtures = [{'id': 'demo-opening-quotes', 'as_of': cutoff, 'known_at': cutoff, 'events': events,
                 'expected': [{'instrument_id': q.instrument_id, 'measure': q.expected_measure, 'value': float(q.expected_value),
                               'tolerance': 1e-8} for q in universe.quotes]}]
    feed = {'source': FEED_SOURCE, 'level_source': LEVEL_SOURCE, 'prices': universe.feed_prices, 'levels': universe.feed_levels,
            'fixings': {'index_id': INDEX_ID, 'source': FIXING_SOURCE, 'rate': str(SOFR),
                        'last_observation': max(f['key']['observation_date'] for f in fixings)}}
    desk = {
        'settlement_date': settlement.isoformat(),
        'marks': [{'instrument_id': q.instrument_id, 'field': q.field, 'level': q.value} for q in universe.quotes if q.source == LEVEL_SOURCE],
        'limits': LIMITS,
        'books': list(BOOKS),
        'rfq_bonds': [b['instrument_id'] for b in universe.bonds if not b['instrument_id'].startswith('UST-')] + ['UST-10Y', 'UST-2Y'],
        'tiers': [tier for tier, _ in TIERS],
    }

    work.mkdir(parents=True, exist_ok=True)
    for name, content in [('seed.json', seed), ('selection.json', selection), ('fixtures.json', fixtures), ('feed.json', feed), ('desk.json', desk)]:
        (work / name).write_text(json.dumps(content, indent=1) + '\n')
    (work / 'events.jsonl').write_text(''.join(json.dumps(event) + '\n' for event in events))
    return {'cutoff': cutoff, 'settlement': settlement.isoformat(), 'bonds': len(universe.bonds)}


if __name__ == '__main__':
    import sys
    print(json.dumps(generate(Path(sys.argv[1]) if len(sys.argv) > 1 else Path('desk-seed'))))
