"""Read-only independent Decimal/Fraction checks; no native pricing imports."""
import datetime
import json
from decimal import Decimal, localcontext
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / 'crates/convex-bonds/tests/fixtures'


def read(name):
    return json.loads((FIXTURES / name).read_bytes())


def exact_text(actual, reference):
    expected = Decimal(reference)
    # The declarations retain110significant digits. This only checks that text
    # precision and is far inside every unchanged financial acceptance bound.
    assert abs(actual - expected) <= Decimal('1e-100') * max(Decimal(1), abs(actual))


with localcontext() as context:
    context.prec = 520
    zero = read('future-zero-instrument.json')['cases']
    assert len(zero) == 60
    for row in zero:
        assert row['independent_discount_years'] == '0'
        for check in row['price_checks']:
            assert Decimal(check['exact_absolute_cash_price']) == Decimal(row['redemption'])
        for check in row['inverse_checks']:
            kind = 'YieldNonUnique' if Decimal(check['absolute_cash_price']) == Decimal(row['redemption']) else 'YieldNoRoot'
            assert check['expected_native_classification'] == kind
    cases = read('zero-instrument-context.json')['cases']
    assert len(cases) == 150 and len({row['day_count'] for row in cases}) == 10
    frequencies = {'Annual': 1, 'SemiAnnual': 2, 'Quarterly': 4, 'Monthly': 12, 'Continuous': None}
    for row in cases:
        fraction = Fraction(365, 360) if row['day_count'] == 'Act360' else Fraction(1)
        assert Fraction(row['discount_years_exact']) == fraction
        time = Decimal(fraction.numerator) / fraction.denominator
        rate = Decimal(row['annual_decimal_yield'])
        frequency = frequencies[row['compounding']]
        continuous = rate if frequency is None else frequency * (1 + rate / frequency).ln()
        exact_text(Decimal(row['redemption']) * (-continuous * time).exp(), row['expected_absolute_price'])
        inverse_continuous = (Decimal(row['redemption']) / Decimal(row['inverse_absolute_price'])).ln() / time
        inverse = inverse_continuous if frequency is None else frequency * ((inverse_continuous / frequency).exp() - 1)
        exact_text(inverse, row['expected_annual_yield'])
        exact_text(inverse_continuous, row['expected_continuous_rate'])
    tail = read('fixed-icma-tail.json')
    date = datetime.date.fromisoformat
    left, end = date(tail['settlement']), date(tail['actual_remaining_payment_date'])
    time = Fraction(0)
    covered = 0
    for reference in tail['original_nominal_mesh']:
        start, finish = date(reference['start']), date(reference['end'])
        days = max(0, (min(end, finish) - max(left, start)).days)
        time += Fraction(days, 2 * (finish - start).days)
        covered += days
    assert covered == (end - left).days and time == Fraction(tail['independent_discount_years_exact']) == Fraction(91, 181)
    years = Decimal(time.numerator) / time.denominator
    rate = Decimal.from_float(float(tail['price_input_binary64_yield']))
    base = 1 + rate / 2
    pv = Decimal('102.5') * (-2 * years * base.ln()).exp()
    exact_text(years, tail['independent_discount_years'])
    exact_text(pv, tail['independent_dirty_price'])
    risk = read('fixed-icma-tail-risk.json')['risk']
    modified = years / base
    convexity = years * (years + Decimal('0.5')) / (base * base)
    for key, value in dict(present_value=pv, macaulay_duration=years,
                           modified_duration=modified, convexity=convexity,
                           dv01_loss=pv * modified * Decimal('0.0001'),
                           first_derivative=-pv * modified, second_derivative=pv * convexity).items():
        exact_text(value, risk[key])
    fixed = read('fixed-zero-time.json')
    assert Decimal(fixed['exact_remaining_coupon']) + Decimal(fixed['exact_remaining_redemption']) == 105
    assert fixed['exact_discount_years'] == '0' and fixed['exact_accrued_per100'] == '5'
    for check in fixed['price_checks']:
        assert Decimal(check['exact_dirty_per100']) == 105 and Decimal(check['exact_clean_per100']) == 100
    for check in fixed['inverse_checks']:
        kind = 'YieldNonUnique' if Decimal(check['clean_per100']) == 100 else 'YieldNoRoot'
        assert check['expected_native_classification'] == kind
print(json.dumps(dict(status='passed', zero_time_instruments=60, complete_context_cases=150,
                      fixed_tail_cases=1, fixed_zero_time_cases=1, native_imported=False,
                      references_or_bounds_written=False, certified=False)))
