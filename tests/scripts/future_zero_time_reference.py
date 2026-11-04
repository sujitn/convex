"""Read-only Decimal520 verification of the supplied zero/one-year reference cases.

These January 30/31 cases use exact 30/360 times0or1, not a general calendar
reference implementation. Exact binary64 rates/targets enter Decimal arithmetic.
No native executable is imported and no expected value or bound is rewritten.
"""
import json
import copy
import sys
from decimal import Decimal as D, localcontext
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'tests/fixtures/library/future-zero-time.json'


def check(actual, expected, bound):
    assert abs(actual - D(expected)) <= D(bound), (str(actual), expected, bound)


def times(action):
    assert action['settlement'] == '2025-01-30'
    basis = action['basis']
    if basis['kind'] == 'calendar':
        assert basis['day_count'] in ['Thirty360US', 'Thirty360E', 'Thirty360German']
    else:
        assert basis == dict(kind='thirty360_e_isda', termination_date='2026-01-31')
    # Each admitted convention changes a final January31to30. Start is January30.
    return [dict(zip(['2025-01-31', '2026-01-31'], [D(0), D(1)], strict=True))[cash['date']]
            for cash in action['cash']]


def law(action):
    value = action['compounding']
    if isinstance(value, str):
        return {'Annual': ('periodic', D(1)), 'SemiAnnual': ('periodic', D(2)),
                'Continuous': ('continuous', None), 'Simple': ('simple', None)}[value]
    kind = value['kind']
    return ('periodic', D(1)) if kind == 'annual' else (
        ('periodic', D.from_float(float(value['frequency']))) if kind == 'periodic' else (kind, None))


def verify():
    source = json.loads(SOURCE.read_bytes())
    assert source['schema_version'] == 1 and source['certified'] is False
    assert len(source['cases']) == 368
    counts = dict(prices=0, risks=0, identified_inverses=0, non_unique=0, no_root=0)
    with localcontext() as context:
        context.prec = 520
        for row in source['cases']:
            action = row['request']['operation']['action']
            actual_times = times(action)
            amounts = [D(cash['amount']) for cash in action['cash']]
            expected = row['expected']
            kind, frequency = law(action)
            if action['kind'] == 'dated_solve':
                target = D.from_float(float(action['target_dirty_price']))
                constant = sum((a for a, t in zip(amounts, actual_times, strict=True) if t == 0), D(0))
                variable = sum((a for a, t in zip(amounts, actual_times, strict=True) if t == 1), D(0))
                if variable == 0 or target <= constant:
                    classification = 'non_unique' if variable == 0 and target == constant else 'no_root'
                    assert expected['structural_root_classification'] == classification
                    assert row['expected_error'] == 'invalid_input'
                    counts[classification] += 1
                    continue
                ratio = variable / (target - constant)
                if kind == 'periodic':
                    rate = frequency * ((ratio.ln() / frequency).exp() - 1)
                elif kind == 'continuous':
                    rate = ratio.ln()
                else:
                    assert kind == 'simple'
                    rate = ratio - 1
                check(rate, expected['annual_yield'], expected['absolute_yield_bound'])
                assert expected['residual_bound'] == '1e-10'
                counts['identified_inverses'] += 1
                continue
            rate = D.from_float(float(action['annual_yield']))
            pv = first = second = weighted = D(0)
            for amount, time in zip(amounts, actual_times, strict=True):
                if kind == 'periodic':
                    base = 1 + rate / frequency
                    df = (-frequency * time * base.ln()).exp()
                    d1 = -time * df / base
                    d2 = time * (time + 1 / frequency) * df / base**2
                elif kind == 'continuous':
                    df = (-rate * time).exp()
                    d1, d2 = -time * df, time**2 * df
                else:
                    assert kind == 'simple'
                    df = 1 / (1 + rate * time)
                    d1, d2 = -time * df**2, 2 * time**2 * df**3
                pv += amount * df
                first += amount * d1
                second += amount * d2
                weighted += amount * df * time
            if action['kind'] == 'dated_price':
                check(pv, expected['present_value'], expected['price_absolute_bound'])
                assert expected['price_absolute_bound'] == '1e-10' and expected['time_absolute_bound'] == '1e-14'
                for time, amount, pair in zip(actual_times, amounts, expected['discount_cash'], strict=True):
                    assert [time, amount] == [D(pair[0]), D(pair[1])]
                counts['prices'] += 1
            else:
                assert action['kind'] == 'dated_risk' and expected['absolute_bound'] == '1e-8'
                risk = dict(present_value=pv, first_derivative=first, second_derivative=second,
                            macaulay_duration=weighted / pv, modified_duration=-first / pv,
                            convexity=second / pv, dv01_loss=-first * D('0.0001'))
                assert set(risk) == set(expected['risk'])
                for field, value in risk.items():
                    check(value, expected['risk'][field], expected['absolute_bound'])
                counts['risks'] += 1
    assert counts == dict(prices=144, risks=96, identified_inverses=48, non_unique=16, no_root=64)
    return dict(status='passed', method='Decimal520; exact binary64 rates/targets; exact0/1times',
                cases=368, **counts, references_and_bounds_rewritten=False, certified=False)


if __name__ == '__main__':
    result = verify()
    if sys.argv[1:]:
        assert len(sys.argv) == 3 and sys.argv[1] == '--schema'
        import jsonschema
        schema = json.loads(Path(sys.argv[2]).read_bytes())
        validator_type = jsonschema.validators.validator_for(schema)
        validator_type.check_schema(schema)
        validator = validator_type(schema)
        rows = json.loads(SOURCE.read_bytes())['cases']
        for row in rows:
            validator.validate(row['request'])
        mutations = []
        for field in ['cash', 'settlement', 'basis', 'compounding', 'annual_yield']:
            request = copy.deepcopy(rows[0]['request'])
            del request['operation']['action'][field]
            mutations.append(request)
        request = copy.deepcopy(rows[0]['request'])
        request['operation']['action']['pretend_paid'] = True
        mutations.append(request)
        assert all(list(validator.iter_errors(request)) for request in mutations)
        result.update(actual_generated_schema_cases=len(rows), rejected_schema_mutations=len(mutations))
    print(json.dumps(result))
