"""The desk demo's synthetic seed, generated for one launch (stdlib only).

Everything here is fictional and labelled demo: the issuers, the bonds, the books, the
curves and the fixings. `generate` writes, into a work directory:

  seed.json       the `convex-core-inputs` file: reference data (bonds and their masters,
                  pricing rules, the Treasury benchmark set, the books), curve recipes,
                  SOFR fixings through the last weekday and the pricing configuration
  selection.json  the `convex-config compile` selection: the books, quote sources, measures
                  and client tiers, settling on the launch's settlement date
  fixtures.json   one activation fixture: a quote for every bond and the value each must
  events.jsonl    give exactly; and its events as the replay file
  feed.json       the feed's instruments, opening prices, sensitivities and fixings

and returns what the launcher sets up once the server runs: the desk's opening marks and
the bonds the RFQ simulator trades.

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
from typing import NamedTuple

getcontext().prec = 50

ROOT = Path(__file__).resolve().parents[2]
CONFIG_ID = 'DESK-USD'
FEED_SOURCE = 'desk-feed'
LEVEL_SOURCE = 'desk'
SET_ID = 'UST-OTR'
INDEX_ID = 'USD.SOFR'  # the projection recipe's index, which the rate index names
FIXING_SOURCE = 'demo-administrator'
FIXING_LAG_DAYS = 1  # published the next business day,
FIXING_HOUR_UTC = 13  # at 13:00 UTC
SOFR = Decimal('0.0430')
CALENDAR = {'id': 'cal-sifma', 'version': 1}
EFFECTIVE = {'from': '2020-01-01T00:00:00Z', 'until': None}
PROVENANCE = {'source': 'desk-demo', 'evidence_reference': 'synthetic'}
TIERS = {'1': '0.75', '2': '1', '3': '1.5'}  # client tier: width factor
MEASURES = [
    'clean_price_mid', 'dirty_price_mid', 'accrued_interest', 'ytm_mid', 'ytw_mid',
    'years_to_worst_mid', 'modified_duration_mid', 'convexity_mid', 'dv01_mid', 'cs01_mid',
    'z_spread_mid', 'g_spread_mid', 'discount_margin_mid', 'factor',
    'krd_2y_mid', 'krd_5y_mid', 'krd_10y_mid', 'krd_30y_mid',
]
# A Treasury pays twice a year.
TREASURY_FREQUENCY = 2
TREASURY_PERIOD_MONTHS = 12 // TREASURY_FREQUENCY


# ---------------------------------------------------------------------------------------
# Dates
# ---------------------------------------------------------------------------------------

def add_months(day: date, months: int) -> date:
    """`day` moved by whole months; the day of month is at most 28 here, so it always exists."""
    index = day.year * 12 + day.month - 1 + months
    return date(index // 12, index % 12 + 1, day.day)


def observed(day: date) -> date:
    """A fixed holiday on a weekend is observed on the Friday before or the Monday after."""
    return day - timedelta(days=1) if day.weekday() == 5 else day + timedelta(days=1) if day.weekday() == 6 else day


def nth_weekday(year: int, month: int, weekday: int, nth: int) -> date:
    first = date(year, month, 1)
    return first + timedelta(days=(weekday - first.weekday()) % 7 + 7 * (nth - 1))


def last_weekday(year: int, month: int, weekday: int) -> date:
    last = add_months(date(year, month, 1), 1) - timedelta(days=1)
    return last - timedelta(days=(last.weekday() - weekday) % 7)


def easter(year: int) -> date:
    """Western Easter Sunday (the anonymous Gregorian computus)."""
    a, b, c = year % 19, year // 100, year % 100
    d, e = divmod(b, 4)
    g = (8 * b + 13) // 25
    h = (19 * a + b - d - g + 15) % 30
    i, k = divmod(c, 4)
    m = (32 + 2 * e + 2 * i - h - k) % 7
    days = h + m - 7 * ((a + 11 * h + 19 * m) // 433)
    month = (days + 90) // 25
    return date(year, month, (days + 33 * month + 19) % 32)


def sifma_holidays(year: int) -> set[date]:
    """The SIFMA holidays of `year`, as the server's calendar holds them."""
    monday, thursday = 0, 3
    holidays = {observed(date(year, 1, 1)), nth_weekday(year, 1, monday, 3), nth_weekday(year, 2, monday, 3),
                easter(year) - timedelta(days=2), last_weekday(year, 5, monday), observed(date(year, 7, 4)),
                nth_weekday(year, 9, monday, 1), nth_weekday(year, 10, monday, 2), observed(date(year, 11, 11)),
                nth_weekday(year, 11, thursday, 4), observed(date(year, 12, 25))}
    if year >= 2021:
        holidays.add(observed(date(year, 6, 19)))
    return holidays


def business_day_on_or_after(day: date) -> date:
    """`day`, or the next SIFMA business day after it."""
    holidays = sifma_holidays(day.year) | sifma_holidays(day.year + 1)
    while day.weekday() >= 5 or day in holidays:
        day += timedelta(days=1)
    return day


def stamp(at: datetime) -> str:
    return at.isoformat(timespec='milliseconds').replace('+00:00', 'Z')


# ---------------------------------------------------------------------------------------
# The independent Treasury reference
# ---------------------------------------------------------------------------------------

def street_clean_price(coupon: Decimal, issue: date, maturity: date, settlement: date, ytm: Decimal) -> Decimal:
    """A Treasury's clean price per 100 at `ytm`, compounded semiannually, by the street
    convention: paid unadjusted on its regular semiannual schedule from `issue`, accruing
    ACT/ACT ICMA, each flow discounted over its whole periods plus the fraction of the
    current period still to run."""
    coupons = []
    day = add_months(issue, TREASURY_PERIOD_MONTHS)
    while day <= maturity:
        coupons.append(day)
        day = add_months(day, TREASURY_PERIOD_MONTHS)
    assert coupons and coupons[-1] == maturity, 'a regular schedule'
    upcoming = [day for day in coupons if day > settlement]
    previous = add_months(upcoming[0], -TREASURY_PERIOD_MONTHS)
    period = Decimal((upcoming[0] - previous).days)
    to_run = Decimal((upcoming[0] - settlement).days) / period
    per_period = coupon * 100 / TREASURY_FREQUENCY
    growth = 1 + ytm / TREASURY_FREQUENCY
    dirty = Decimal(0)
    for index in range(len(upcoming)):
        flow = per_period + (100 if index == len(upcoming) - 1 else 0)
        dirty += flow / growth ** (Decimal(index) + to_run)
    accrued = per_period * Decimal((settlement - previous).days) / period
    return dirty - accrued


def modified_duration(coupon: Decimal, issue: date, maturity: date, settlement: date, ytm: Decimal, price: Decimal) -> float:
    """By a one basis point central difference of the street price at `price`: for the feed's moves only."""
    bump = Decimal('0.0001')
    up = street_clean_price(coupon, issue, maturity, settlement, ytm + bump)
    down = street_clean_price(coupon, issue, maturity, settlement, ytm - bump)
    return round(float((down - up) / (2 * bump * price)), 6)


def percent(rate: Decimal) -> str:
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

def settings(day_count: str, business_day_convention: str, settlement_days: int) -> dict:
    return {'day_count': day_count, 'business_day_convention': business_day_convention,
            'end_of_month': False, 'settlement_days': settlement_days, 'ex_coupon_days': 0}


CONVENTIONS = {
    # Treasuries pay unadjusted, so the street yield is the reference's textbook formula.
    'convention-ust': settings('ActActIcma', 'Unadjusted', 1),
    'convention-corporate': settings('Thirty360US', 'Following', 2),
    'convention-sinker': settings('Thirty360US', 'Unadjusted', 2),
    'convention-frn': settings('Act360', 'Following', 2),
}
DAY_COUNT_NAMES = {'ActActIcma': 'ACT/ACT ICMA', 'Thirty360US': '30/360 US', 'Act360': 'ACT/360'}


class Issuer(NamedTuple):
    legal_name: str
    short_name: str
    issuer_type: str
    sector: str


ISSUERS = {
    'US-TREASURY': Issuer('United States Treasury (synthetic)', 'UST', 'Sovereign', 'Government'),
    'HARBOR-UTIL': Issuer('Harbor Point Utilities (fictional)', 'Harbor Point Utilities', 'CorporateIG', 'Utilities'),
    'GRANITE-ENERGY': Issuer('Granite Ridge Energy (fictional)', 'Granite Ridge Energy', 'CorporateIG', 'Energy'),
    'BLUEWATER-TEL': Issuer('Bluewater Telecom (fictional)', 'Bluewater Telecom', 'CorporateIG', 'Telecommunications'),
    'SUMMIT-RAIL': Issuer('Summit Rail (fictional)', 'Summit Rail', 'CorporateIG', 'Transportation'),
    'CEDAR-HEALTH': Issuer('Cedar Health (fictional)', 'Cedar Health', 'CorporateHY', 'Healthcare'),
    'IRONWOOD-STEEL': Issuer('Ironwood Steel (fictional)', 'Ironwood Steel', 'CorporateHY', 'Materials'),
    'MERIDIAN-BANK': Issuer('Meridian Bank (fictional)', 'Meridian Bank', 'CorporateIG', 'Financials'),
}


def title(issuer_id: str, coupon: Decimal | None, maturity: date | None, suffix: str) -> str:
    """A bond's description: issuer, coupon and maturity, as a sheet lists it."""
    coupon_text = f' {percent(coupon)}%' if coupon else ''
    maturity_text = f' {maturity:%b-%y}' if maturity else ''
    return f'{ISSUERS[issuer_id].short_name}{coupon_text}{maturity_text}{suffix}'


def bond(instrument_id: str, issuer_id: str, *, issue: date, maturity: date | None, coupon: Decimal | None,
         frequency: int, convention: str, bond_type: str, suffix: str = '', cusip_code: str | None = None,
         seniority: str = 'Senior', payment_lag: int = 0, contractual: dict | None = None, **record: object) -> dict:
    """A bond's reference record, described by its issuer, coupon and maturity and `suffix`;
    `contractual` adds to its contractual terms and `record` to the record."""
    issuer = ISSUERS[issuer_id]
    terms = {
        'effective': EFFECTIVE, 'dated_date': issue.isoformat(), 'penultimate_coupon_date': None,
        'redemption_value': '100', 'issuer': {'entity_id': issuer_id, 'version': 1}, 'calendar': CALENDAR,
        'calendar_code': 'sifma', 'convention': {'id': convention, 'version': 1}, 'settings': CONVENTIONS[convention],
        'provenance': PROVENANCE, 'stub': 'none', 'payment_lag': payment_lag, **(contractual or {}),
    }
    reference = {
        'instrument_id': instrument_id, 'isin': None, 'cusip': cusip_code, 'sedol': None, 'bbgid': None,
        'description': title(issuer_id, coupon, maturity, suffix), 'currency': 'USD', 'issue_date': issue.isoformat(),
        'coupon_rate': None if coupon is None else str(coupon), 'frequency': frequency,
        'day_count': DAY_COUNT_NAMES[CONVENTIONS[convention]['day_count']], 'face_value': '100',
        'bond_type': bond_type, 'issuer_type': issuer.issuer_type, 'issuer_id': issuer_id,
        'issuer_name': issuer.legal_name, 'seniority': seniority, 'is_callable': False, 'call_schedule': [],
        'is_putable': False, 'is_sinkable': False, 'floating_terms': None, 'inflation_index': None,
        'inflation_base_index': None, 'has_deflation_floor': False, 'country_of_risk': 'US',
        'sector': issuer.sector, 'amount_outstanding': '500000000', 'first_coupon_date': None,
        'last_updated': 0, 'source': 'desk-demo', 'contractual_terms': terms, **record,
    }
    if maturity is not None:
        reference['maturity_date'] = maturity.isoformat()
    return reference


def master(kind: str, definition: dict) -> dict:
    return {'expected_version': None, 'definition': {'kind': kind, 'definition': definition}}


def rule(instrument_id: str, method: dict, *, benchmark_set: str | None = None, width: dict | None = None) -> dict:
    definition = {'instrument_id': instrument_id, 'method': method, 'effective': EFFECTIVE, 'provenance': PROVENANCE}
    if benchmark_set:
        definition['benchmark_set_id'] = benchmark_set
    if width:
        definition['width'] = width
    return master('pricing_rule', definition)


def widths(unit: str, small: str, medium: str, large: str) -> dict:
    """A two-way width by trade size: to 1MM, to 10MM, and above."""
    return {'unit': unit, 'buckets': [
        {'max_face': '1000000', 'width': small}, {'max_face': '10000000', 'width': medium}, {'width': large}]}


# ---------------------------------------------------------------------------------------
# The universe
# ---------------------------------------------------------------------------------------

QUOTE_UNITS = {'mid_price': 'price_per100', 'mid_spread': 'spread_basis_points', 'mid_yield': 'yield_decimal'}


@dataclass
class Quote:
    """A bond's opening quote, where it is read from, the value activation expects, and how
    the feed moves it: a price by its modified duration, a level or a price by its credit
    beta."""
    instrument_id: str
    source: str
    field: str
    value: str
    expected_measure: str
    expected_value: Decimal
    credit_beta: float
    modified_duration: float | None = None

    @property
    def unit(self) -> str:
        return QUOTE_UNITS[self.field]


@dataclass
class Universe:
    settlement: date
    bonds: list = field(default_factory=list)
    rules: list = field(default_factory=list)
    quotes: list = field(default_factory=list)
    benchmark_set: dict | None = None
    benchmark_ids: list = field(default_factory=list)


class Treasury(NamedTuple):
    tenor: str
    years: int
    coupon: str
    opening_yield: str


TREASURIES = [
    Treasury('2Y', 2, '0.03875', '0.0395'),
    Treasury('3Y', 3, '0.03875', '0.0398'),
    Treasury('5Y', 5, '0.0400', '0.0408'),
    Treasury('7Y', 7, '0.04125', '0.0418'),
    Treasury('10Y', 10, '0.0425', '0.0430'),
    Treasury('20Y', 20, '0.04625', '0.0464'),
    Treasury('30Y', 30, '0.04625', '0.0468'),
]


def add_treasuries(universe: Universe) -> dict[str, Decimal]:
    """The benchmark set, each Treasury quoted by price at its opening street yield; each
    tenor's opening yield."""
    settlement = universe.settlement
    # Issued on the latest 15th on or before settlement, so each pays regularly on the 15th
    # and settlement is never a coupon date, which the reference's schedule assumes.
    issue = settlement.replace(day=15) if settlement.day >= 15 else add_months(settlement.replace(day=15), -1)
    opening_yields: dict[str, Decimal] = {}
    securities = []
    for index, treasury in enumerate(TREASURIES):
        maturity = add_months(issue, 12 * treasury.years)
        coupon, ytm = Decimal(treasury.coupon), Decimal(treasury.opening_yield)
        instrument_id = f'UST-{treasury.tenor}'
        # Issue letters skip I and O, which CUSIPs do not use.
        code = cusip(f'91282C{"LMNPQRS"[index]}{index}')
        universe.bonds.append(bond(instrument_id, 'US-TREASURY',
                                   issue=issue, maturity=maturity, coupon=coupon, frequency=TREASURY_FREQUENCY,
                                   convention='convention-ust', bond_type='FixedBullet', cusip_code=code))
        price = street_clean_price(coupon, issue, maturity, settlement, ytm).quantize(Decimal('1e-12'), ROUND_HALF_EVEN)
        universe.rules.append(rule(instrument_id, {'kind': 'price'}, width={'unit': 'thirty_seconds', 'buckets': [
            {'max_face': '10000000', 'width': '0.5'}, {'width': '1'}]}))
        universe.quotes.append(Quote(instrument_id, FEED_SOURCE, 'mid_price', str(price),
                                     'clean_price_mid', price, credit_beta=0.0,
                                     modified_duration=modified_duration(coupon, issue, maturity, settlement, ytm, price)))
        opening_yields[treasury.tenor] = ytm
        securities.append({'tenor': treasury.tenor, 'instrument_id': instrument_id, 'version': 1})
    universe.benchmark_set = master('benchmark_set', {'benchmark_set_id': SET_ID, 'currency': 'USD',
                                                      'securities': securities, 'effective': EFFECTIVE,
                                                      'provenance': PROVENANCE})
    universe.benchmark_ids = [security['instrument_id'] for security in securities]
    return opening_yields


def corporate_issue(settlement: date, years_ago: int) -> date:
    """An issue date `years_ago` before the latest roll day on or before settlement. The roll
    day is one no coupon or sink shares with settlement, so the sinkers' factor and the
    fixture's echoed quotes need no boundary convention."""
    anchor = settlement.replace(day=10 if settlement.day != 10 else 11)
    if anchor > settlement:
        anchor = add_months(anchor, -1)
    return add_months(anchor, -12 * years_ago)


def add_investment_grade(universe: Universe, opening_yields: dict[str, Decimal]) -> None:
    """Bullets on a spread to the Treasury of their tenor, each expected at that Treasury's
    street yield plus its spread."""
    for instrument_id, issuer_id, coupon, years_ago, term, tenor, spread in [
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
        issue = corporate_issue(universe.settlement, years_ago)
        maturity = add_months(issue, 12 * term)
        universe.bonds.append(bond(instrument_id, issuer_id, issue=issue,
                                   maturity=maturity, coupon=Decimal(coupon), frequency=2,
                                   convention='convention-corporate', bond_type='FixedBullet'))
        universe.rules.append(rule(instrument_id, {'kind': 'benchmark_spread', 'tenor': tenor},
                                   benchmark_set=SET_ID, width=widths('basis_points', '2', '4', '8')))
        universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_spread', spread,
                                     'ytm_mid', opening_yields[tenor] + Decimal(spread) / 10000, credit_beta=1.0))


def add_high_yield(universe: Universe) -> None:
    """Callables, two quoted by price and two by yield to worst; mid only, as a width
    applies to bullets."""
    for instrument_id, issuer_id, coupon, term, first_call, method, level in [
        ('HY-CDH-31', 'CEDAR-HEALTH', '0.0675', 7, 2, 'price', '101.25'),
        ('HY-CDH-33', 'CEDAR-HEALTH', '0.0725', 8, 3, 'yield', '0.0705'),
        ('HY-IRS-30', 'IRONWOOD-STEEL', '0.0800', 6, 1, 'price', '102.5'),
        ('HY-IRS-32', 'IRONWOOD-STEEL', '0.0850', 8, 2, 'yield', '0.0790'),
    ]:
        issue = corporate_issue(universe.settlement, 2)
        maturity = add_months(issue, 12 * term)
        # Annual calls from the first, at a premium stepping down to par.
        premium = Decimal(coupon) * 50
        calls = [{'call_date': add_months(issue, 12 * year).isoformat(),
                  'call_price': str(max(Decimal(100), 100 + premium * (term - 1 - year) / max(1, term - 1 - first_call)).quantize(Decimal('0.001'))),
                  'is_make_whole': False} for year in range(first_call, term)]
        universe.bonds.append(bond(instrument_id, issuer_id, suffix=' callable', issue=issue,
                                   maturity=maturity, coupon=Decimal(coupon), frequency=2, convention='convention-corporate',
                                   bond_type='FixedCallable', contractual={'call_terms': {'style': 'bermudan', 'notice_days': 30}},
                                   is_callable=True, call_schedule=calls))
        universe.rules.append(rule(instrument_id, {'kind': method}))
        if method == 'price':
            universe.quotes.append(Quote(instrument_id, FEED_SOURCE, 'mid_price', level, 'clean_price_mid',
                                         Decimal(level), credit_beta=2.5, modified_duration=3.0))
        else:
            universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_yield', level, 'ytw_mid',
                                         Decimal(level), credit_beta=2.5))


def add_floaters(universe: Universe) -> None:
    """SOFR notes quoted by discount margin."""
    for instrument_id, issuer_id, quoted_margin, term, margin in [
        ('FRN-MER-29', 'MERIDIAN-BANK', '65', 3, '58'),
        ('FRN-HPU-30', 'HARBOR-UTIL', '80', 4, '74'),
    ]:
        issue = corporate_issue(universe.settlement, 1)
        maturity = add_months(issue, 12 * term)
        universe.bonds.append(bond(instrument_id, issuer_id, suffix=f' SOFR+{quoted_margin}',
                                   issue=issue, maturity=maturity, coupon=None, frequency=4, convention='convention-frn',
                                   bond_type='FloatingRate', payment_lag=2,
                                   contractual={'floating_mechanics': {'index': {'id': 'usd-sofr', 'version': 1}, 'overrides': {}}},
                                   floating_terms={'index': 'Sofr', 'spread': quoted_margin, 'reset_frequency': 4, 'cap': None, 'floor': None}))
        universe.rules.append(rule(instrument_id, {'kind': 'discount_margin'}))
        universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_spread', margin,
                                     'discount_margin_mid', Decimal(margin), credit_beta=0.5))


def add_sinkers(universe: Universe) -> None:
    """Sinking-fund bonds by price per 100 current face, expected at their price times their factor."""
    for instrument_id, issuer_id, coupon, term, level in [
        ('SNK-SRL-34', 'SUMMIT-RAIL', '0.0560', 12, '99.5'),
        ('SNK-GRE-37', 'GRANITE-ENERGY', '0.0600', 14, '101.0'),
    ]:
        issue = corporate_issue(universe.settlement, 4)
        maturity = add_months(issue, 12 * term)
        # Eight per cent of the original face a year from the third, the rest at maturity.
        sinks = [{'date': add_months(issue, 12 * year).isoformat(), 'amount': '8', 'price': '100'} for year in range(3, term - 2)]
        factor = 1 - sum(Decimal(sink['amount']) for sink in sinks if date.fromisoformat(sink['date']) < universe.settlement) / 100
        universe.bonds.append(bond(instrument_id, issuer_id, suffix=' sinker', issue=issue,
                                   maturity=maturity, coupon=Decimal(coupon), frequency=2, convention='convention-sinker',
                                   bond_type='Amortizing', contractual={'sinking_schedule': sinks}, is_sinkable=True))
        universe.rules.append(rule(instrument_id, {'kind': 'price'}))
        universe.quotes.append(Quote(instrument_id, FEED_SOURCE, 'mid_price', level, 'clean_price_mid',
                                     Decimal(level) * factor, credit_beta=1.0, modified_duration=5.0))


def add_perpetuals(universe: Universe) -> None:
    """Callable perpetuals quoted by yield to worst."""
    for instrument_id, issuer_id, coupon, first_call_years, level in [
        ('PERP-MER', 'MERIDIAN-BANK', '0.0650', 4, '0.0610'),
        ('PERP-BWT', 'BLUEWATER-TEL', '0.0700', 3, '0.0675'),
    ]:
        issue = corporate_issue(universe.settlement, 1)
        first_call = add_months(issue, 12 * first_call_years)
        universe.bonds.append(bond(instrument_id, issuer_id, suffix=' perpetual', issue=issue,
                                   maturity=None, coupon=Decimal(coupon), frequency=2, convention='convention-corporate',
                                   bond_type='Perpetual', seniority='Subordinated',
                                   contractual={'perpetual': {'first_call_date': first_call.isoformat(), 'call_interval_months': 6,
                                                              'call_price': '100', 'notice_days': 30}},
                                   is_callable=True))
        universe.rules.append(rule(instrument_id, {'kind': 'yield'}))
        universe.quotes.append(Quote(instrument_id, LEVEL_SOURCE, 'mid_yield', level, 'ytw_mid',
                                     Decimal(level), credit_beta=2.0))


def build(settlement: date) -> Universe:
    universe = Universe(settlement)
    opening_yields = add_treasuries(universe)
    add_investment_grade(universe, opening_yields)
    add_high_yield(universe)
    add_floaters(universe)
    add_sinkers(universe)
    add_perpetuals(universe)
    return universe


class Position(NamedTuple):
    instrument_id: str
    face: str
    rating: str


BOOKS = {
    'BOOK-IG': ('Investment grade credit', [
        Position('IG-HPU-30', '5000000', 'A-'), Position('IG-HPU-35', '3000000', 'A-'),
        Position('IG-GRE-29', '4000000', 'BBB+'), Position('IG-GRE-34', '2000000', 'BBB+'),
        Position('IG-BWT-31', '6000000', 'BBB'), Position('IG-BWT-55', '2000000', 'BBB'),
        Position('IG-SRL-33', '3000000', 'A'), Position('IG-SRL-45', '2000000', 'A'),
        Position('IG-MER-29', '5000000', 'A+'), Position('IG-MER-36', '3000000', 'A+'),
        Position('UST-5Y', '-8000000', 'AA+'), Position('UST-10Y', '-5000000', 'AA+'),
    ]),
    'BOOK-HY': ('High yield, sinkers and perpetuals', [
        Position('HY-CDH-31', '4000000', 'BB-'), Position('HY-CDH-33', '3000000', 'BB-'),
        Position('HY-IRS-30', '2000000', 'B+'), Position('HY-IRS-32', '2000000', 'B+'),
        Position('SNK-SRL-34', '3000000', 'A'), Position('SNK-GRE-37', '2000000', 'BBB+'),
        Position('PERP-MER', '2000000', 'BBB-'), Position('PERP-BWT', '1000000', 'BB+'),
    ]),
    'BOOK-FLOW': ('Rates and floaters', [
        Position('UST-2Y', '10000000', 'AA+'), Position('UST-3Y', '5000000', 'AA+'),
        Position('UST-7Y', '-3000000', 'AA+'), Position('UST-20Y', '2000000', 'AA+'),
        Position('UST-30Y', '-1000000', 'AA+'), Position('FRN-MER-29', '8000000', 'A+'),
        Position('FRN-HPU-30', '5000000', 'A-'),
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


# ---------------------------------------------------------------------------------------
# The files
# ---------------------------------------------------------------------------------------

def references(universe: Universe, recorded: str) -> list:
    """The reference commands: masters, bonds, rules with the benchmark set, and books."""
    held = {position.instrument_id for _, positions in BOOKS.values() for position in positions}
    unheld = {b['instrument_id'] for b in universe.bonds} - held - set(universe.benchmark_ids)
    assert not unheld, f'every bond is held or a benchmark: {unheld}'
    entities = [master('entity', {'entity_id': issuer_id, 'legal_name': issuer.legal_name, 'parent': None, 'country': 'US',
                                  'effective': EFFECTIVE, 'provenance': PROVENANCE}) for issuer_id, issuer in ISSUERS.items()]
    calendar = master('calendar', {'calendar_id': CALENDAR['id'], 'code': 'sifma', 'rule_version': 'convex-calendar-v1',
                                   'coverage_from': '1990-01-01', 'coverage_until': '2100-12-31',
                                   'effective': EFFECTIVE, 'provenance': PROVENANCE})
    rate_index = master('rate_index', {
        'rate_index_id': 'usd-sofr', 'index_id': INDEX_ID, 'currency': 'USD', 'kind': 'overnight',
        'fixing_source': FIXING_SOURCE, 'calendar': CALENDAR, 'calendar_code': 'sifma',
        'publication_lag_business_days': FIXING_LAG_DAYS, 'publication_utc_seconds': FIXING_HOUR_UTC * 3600,
        'conventions': {'lookback_business_days': 2, 'observation_shift': False, 'lockout_business_days': 0},
        'effective': EFFECTIVE, 'provenance': PROVENANCE})
    conventions = [master('convention', {'convention_id': convention_id, 'settings': convention, 'effective': EFFECTIVE,
                                         'provenance': PROVENANCE}) for convention_id, convention in CONVENTIONS.items()]
    books = [master('book', {
        'book_id': book_id, 'owning_system': 'desk-bms', 'name': name, 'currency': 'USD', 'description': 'Synthetic demo book',
        'effective': {'from': recorded, 'until': None},
        'positions': [{'instrument_id': position.instrument_id, 'quantity': position.face, 'unit': 'face',
                       'reference_version': None, 'source_identifier': None, 'sector': None, 'rating': position.rating}
                      for position in positions],
        'provenance': PROVENANCE}) for book_id, (name, positions) in BOOKS.items()]
    commands = [
        ('demo-masters', [], entities + [calendar, rate_index] + conventions),
        ('demo-bonds', [{'kind': 'create', 'bond': b} for b in universe.bonds], []),
        ('demo-rules', [], universe.rules + [universe.benchmark_set]),
        ('demo-books', [], books),
    ]
    return [{'recorded_at': recorded, 'command': {'idempotency_key': key, 'changes': changes, 'master_changes': masters}}
            for key, changes, masters in commands]


def showcase_inputs(recorded: datetime) -> tuple[list, dict]:
    """The showcase's discount and SOFR projection recipes, recorded fresh, and its pricing
    configuration as `DESK-USD`, which also projects floaters on SOFR."""
    showcase = json.loads((ROOT / 'deploy/showcase/seed.json').read_text())
    curves = []
    for curve in showcase['curves']:
        if curve['curve_id'] in ('D.OIS', 'P.SOFR'):
            fresh = copy.deepcopy(curve)
            fresh['source_time'] = fresh['recorded_at'] = stamp(recorded)
            curves.append(fresh)
    assert INDEX_ID in {c['recipe']['role'].get('index_id') for c in curves}, 'the projection recipe names the index'
    recorded_ms = int(recorded.timestamp() * 1000)
    pricing = copy.deepcopy(showcase['pricing_configs'][0])
    pricing.update(config_id=CONFIG_ID, description='Desk demo: synthetic USD bonds', created_at=recorded_ms, updated_at=recorded_ms)
    pricing['analytics_curves']['frn_reference_curve'] = 'P.SOFR'
    return curves, pricing


def sofr_fixings(settlement: date, recorded: str) -> list:
    """SOFR for each weekday before settlement, from beyond the floaters' current periods."""
    fixings = []
    day = settlement - timedelta(days=200)
    while day < settlement:
        if day.weekday() < 5:
            fixings.append({'schema_version': 1, 'key': {'index_id': INDEX_ID, 'observation_date': day.isoformat(), 'source': FIXING_SOURCE},
                            'version': 1, 'rate': {'unit': 'decimal_rate', 'value': str(SOFR)}, 'published_at': recorded,
                            'recorded_at': recorded, 'is_preliminary': False, 'entitled': True, 'evidence_reference': 'synthetic'})
        day += timedelta(days=1)
    return fixings


def opening_events(universe: Universe, now: datetime) -> list:
    now_ms = int(now.timestamp() * 1000)
    return [{'schema_version': 1, 'key': {'instrument_id': q.instrument_id, 'source': q.source}, 'session': 'demo-fixture',
             'sequence': 1, 'source_time_ms': now_ms, 'receive_time_ms': now_ms, 'quality': 'live', 'entitled': True,
             'entitlement_tags': [], 'evidence_reference': None,
             'event': {'kind': 'image', 'fields': [{'field': q.field, 'update': {'action': 'set', 'value': {'value': q.value, 'unit': q.unit}}}]}}
            for q in universe.quotes]


def feed_plan(universe: Universe, last_fixing: str) -> dict:
    """The feed's priced bonds, the desk's marked ones, and the index whose fixings it publishes."""
    return {
        'source': FEED_SOURCE, 'level_source': LEVEL_SOURCE,
        'prices': [{'instrument_id': q.instrument_id, 'price': q.value, 'modified_duration': q.modified_duration,
                    'credit_beta': q.credit_beta} for q in universe.quotes if q.source == FEED_SOURCE],
        'levels': [{'instrument_id': q.instrument_id, 'field': q.field, 'credit_beta': q.credit_beta}
                   for q in universe.quotes if q.source == LEVEL_SOURCE],
        'fixings': {'index_id': INDEX_ID, 'source': FIXING_SOURCE, 'rate': str(SOFR), 'last_observation': last_fixing,
                    'publication_lag_business_days': FIXING_LAG_DAYS, 'publication_hour_utc': FIXING_HOUR_UTC},
    }


@dataclass
class Launch:
    """What the launcher sets up once the server runs."""
    settlement: date
    instrument_count: int
    marks: list
    rfq_bonds: list


def generate(work: Path, now: datetime | None = None) -> Launch:
    """Write the seed for a launch at `now` into `work`."""
    now = (now or datetime.now(timezone.utc)).replace(microsecond=0)
    recorded_at = now - timedelta(seconds=30)
    recorded = stamp(recorded_at)
    settlement = business_day_on_or_after(now.date())
    universe = build(settlement)
    curves, pricing = showcase_inputs(recorded_at)
    fixings = sofr_fixings(settlement, recorded)
    seed = {'schema_version': 1, 'references': references(universe, recorded), 'curves': curves,
            'fixings': fixings, 'pricing_configs': [pricing]}
    cutoff = stamp(now)
    selection = {
        'schema_version': 1, 'pricing_config': CONFIG_ID, 'books': list(BOOKS), 'source': FEED_SOURCE, 'fields': ['mid_price'],
        'quote_max_age_ms': 600000, 'max_source_skew_ms': 600000, 'as_of': cutoff, 'known_at': cutoff, 'mode': 'live',
        'settlement_date': settlement.isoformat(), 'measures': MEASURES,
        'numerical_settings': {'policy_id': 'convex-bond-router-v1', 'bump_bps': 1.0}, 'max_concurrent_calcs': 4,
        'levels': {'source': LEVEL_SOURCE, 'max_age_ms': 86400000},
        'client_tiers': [{'client_tier': tier, 'width_factor': factor} for tier, factor in TIERS.items()],
    }
    events = opening_events(universe, now)
    fixtures = [{'id': 'demo-opening-quotes', 'as_of': cutoff, 'known_at': cutoff, 'events': events,
                 'expected': [{'instrument_id': q.instrument_id, 'measure': q.expected_measure, 'value': float(q.expected_value),
                               'tolerance': 1e-8} for q in universe.quotes]}]
    feed = feed_plan(universe, max(f['key']['observation_date'] for f in fixings))

    work.mkdir(parents=True, exist_ok=True)
    for name, content in [('seed.json', seed), ('selection.json', selection), ('fixtures.json', fixtures), ('feed.json', feed)]:
        (work / name).write_text(json.dumps(content, indent=1) + '\n')
    (work / 'events.jsonl').write_text(''.join(json.dumps(event) + '\n' for event in events))
    return Launch(
        settlement=settlement,
        instrument_count=len(universe.bonds),
        marks=[{'instrument_id': q.instrument_id, 'field': q.field, 'level': q.value}
               for q in universe.quotes if q.source == LEVEL_SOURCE],
        rfq_bonds=[b['instrument_id'] for b in universe.bonds if b['instrument_id'] not in universe.benchmark_ids]
        + ['UST-10Y', 'UST-2Y'],
    )


if __name__ == '__main__':
    import sys
    launch = generate(Path(sys.argv[1]) if len(sys.argv) > 1 else Path('desk-seed'))
    print(json.dumps({'settlement': launch.settlement.isoformat(), 'bonds': launch.instrument_count}))
