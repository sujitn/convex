"""Independent dated profile cash controls; no candidate output or calendar lookup."""
import argparse
from datetime import date, timedelta
from decimal import Decimal, localcontext
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / 'tests/fixtures/library/native-primitives-reference.json'


def generate():
    profiles = {
        'coupon': [('2025-01-01', '2025-07-01', '0.01')],
        'spread': [('2025-01-01', '2025-02-01', '0.001'),
                   ('2025-02-01', '2025-05-01', '0.002'),
                   ('2025-05-01', '2025-07-01', '0.003')],
        'gearing': [('2025-01-01', '2025-03-01', '1.5'),
                    ('2025-03-01', '2025-07-01', '2')],
        'notional': [('2025-01-01', '2025-02-15', '100'),
                     ('2025-02-15', '2025-06-15', '80'),
                     ('2025-06-15', '2025-07-01', '50')],
    }

    def value(kind, point):
        return Decimal(next(v for start, end, v in profiles[kind] if start <= point < end))

    def payment(point):
        cursor = date.fromisoformat(point) + timedelta(days=1)
        while cursor.weekday() >= 5:
            cursor += timedelta(days=1)
        return cursor.isoformat()

    cases = []
    with localcontext() as ctx:
        ctx.prec = 80
        for forecast in ['0.05', '0.055', '-0.0025']:
            coupons = []
            for start, end, rate in [('2025-01-01', '2025-04-01', '0.04'),
                                      ('2025-04-01', '2025-07-01', forecast)]:
                boundaries = sorted({start, end} | {p for values in profiles.values()
                                                     for a, b, _ in values for p in (a, b)
                                                     if start < p < end})
                amount = Decimal(0)
                for a, b in zip(boundaries, boundaries[1:]):
                    days = Decimal((date.fromisoformat(b) - date.fromisoformat(a)).days)
                    annual_rate = value('coupon', a) + value('spread', a) + value('gearing', a) * Decimal(rate)
                    amount += value('notional', a) * annual_rate * days / Decimal(360)
                coupons.append(dict(start=start, end=end, payment_date=payment(end), amount=str(amount)))
            cases.append(dict(forecast_rate=forecast, coupons=coupons))
    return dict(schema_version=1, certified=False, currency='USD', cash_kind='projected_contractual',
                calendar='explicit_weekends_only', day_count='Act360', absolute_cash_bound='0.000000000000000000000001',
                valuation_date='2025-02-15', known_at='2025-02-15T12:00:00Z',
                profiles={k: [dict(start=a, end=b, value=v) for a, b, v in values] for k, values in profiles.items()},
                cases=cases, principal=[dict(effective_date=p, payment_date=payment(p), amount=v)
                                       for p, v in [('2025-02-15', '20'), ('2025-06-15', '30'), ('2025-07-01', '50')]],
                reference='Independent Decimal80 sum of exact dated profile slices; supplied publication/history and forecast cases; no native outputs.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['write', 'check'])
    args = parser.parse_args()
    raw = (json.dumps(generate(), indent=2) + '\n').encode()
    if args.mode == 'write':
        if TARGET.exists():
            raise ValueError('Refuse declared reference overwrite')
        TARGET.write_bytes(raw)
    elif TARGET.read_bytes() != raw:
        raise ValueError('Independent dated reference changed')
    print(json.dumps(dict(status='declared' if args.mode == 'write' else 'passed', coupon_controls=6,
                          principal_controls=3, cash_bound='1e-24', certified=False)))
