#!/usr/bin/env python3
"""Pinned CVX-019 swap comparison. Python 3.11+, stdlib; QuantLib only in capture.

The fixed suite contains inputs, never golden external outputs. `run` builds
Convex and executes checksum-verified QuantLib in a separate supplied Python.
`replay` checks the saved bytes, runtime/source bindings and declared bounds.
No mode installs software or grants task/library/production certification.
"""
from __future__ import annotations

import argparse
import datetime as dt
from decimal import Decimal, InvalidOperation
import hashlib
import importlib.metadata
import importlib.util
import json
import math
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
import zipfile

ROOT = Path(__file__).resolve().parents[2]
INPUT = ROOT / 'tests/fixtures/comparisons/swap-quantlib-inputs.json'
INPUT_SHA256 = 'ba61375e251884208f70585cb0aeac034324016cc91623ec7abd0e0020b2211a'
EXPORT = 'crates/convex/examples/swap_comparison.rs'
SCRIPT = 'tests/scripts/swap_quantlib_compare.py'
REFERENCE = ROOT / 'tests/fixtures/comparisons/swap-quantlib-1.40'
CASE_IDS = (
    'irs-seasoned', 'irs-amortizing-stepped', 'irs-negative', 'irs-forward',
    'irs-receiver', 'ois-seasoned', 'ois-amortizing-stepped', 'ois-lookback',
    'ois-shift', 'ois-lockout',
)
BOUNDS = {
    'currency_abs': '0.0000001', 'rate_abs': '0.000000000001',
    'discount_ratio_abs': '0.0000000000001',
    'year_fraction_abs': '0.0000000000001', 'relative': '0',
}


class Failure(ValueError):
    """Explicit input, provenance or numerical rejection, including under -O."""


def require(condition, message):
    if not condition:
        raise Failure(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f'duplicate JSON key: {key}')
        result[key] = value
    return result


def loads(data):
    require(len(data) <= 4 * 1024 * 1024, 'JSON exceeds comparison work bound')
    def reject_constant(value):
        raise Failure(f'nonfinite JSON: {value}')

    return json.loads(data, object_pairs_hook=unique_object, parse_constant=reject_constant)


def write(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def inputs(data=None):
    raw = INPUT.read_bytes() if data is None else data
    require(digest(raw) == INPUT_SHA256, 'pinned comparison inputs changed')
    fixture = loads(raw)
    require(fixture['schema_version'] == 1 and fixture['certified'] is False
            and fixture['qualification'] == 'pinned_inputs_external_capture_pending', 'input qualification drift')
    require(tuple(c['id'] for c in fixture['cases']) == CASE_IDS, 'case scope drift')
    require(fixture['tolerances'] == BOUNDS, 'predeclared comparison bound drift')
    require(all('expected' not in c for c in fixture['cases']), 'inputs must not contain comparator outputs')
    return fixture


def decimal(value):
    require(isinstance(value, str) and 0 < len(value) <= 96, 'output numbers must be finite decimal strings')
    try:
        result = Decimal(value)
    except InvalidOperation as error:
        raise Failure('invalid output decimal') from error
    require(result.is_finite() and abs(result) <= Decimal('1e12'), 'nonfinite/out-of-domain output')
    return result


def number(value):
    require(math.isfinite(value), 'nonfinite QuantLib output')
    return format(value, '.17g')


def verified_runtime(wheel, fixture):
    """Verify installed publisher bytes before importing any QuantLib code."""
    pin = fixture['comparator']
    require(wheel.is_file() and wheel.name == pin['wheel'], 'missing exact pinned QuantLib wheel')
    require(wheel.stat().st_size <= 100 * 1024 * 1024, 'wheel exceeds work bound')
    require(digest(wheel.read_bytes()) == pin['sha256'], 'publisher wheel checksum mismatch')
    require(platform.system() == 'Darwin' and platform.machine() == 'arm64',
            'this runtime pin is macOS ARM64; another platform needs a reviewed publisher pin')
    distribution = importlib.metadata.distribution('QuantLib')
    require(distribution.version == pin['version'], 'installed QuantLib distribution version mismatch')
    files = {}
    with zipfile.ZipFile(wheel) as archive:
        entries = [entry for entry in archive.infolist() if not entry.is_dir()]
        require(len(entries) <= 4096 and sum(e.file_size for e in entries) <= 512 * 1024 * 1024,
                'wheel contents exceed work bound')
        require(len({e.filename for e in entries}) == len(entries), 'duplicate wheel member')
        for entry in entries:
            name = entry.filename
            require(not Path(name).is_absolute() and '..' not in Path(name).parts,
                    'unsupported wheel layout')
            if name.endswith('.dist-info/RECORD') or name.endswith('/RECORD'):
                continue  # pip writes installed RECORD; all publisher payloads are checked below.
            # The publisher places its license in the wheel's data scheme;
            # pip installs that exact member under the supplied venv prefix.
            license_member = f"quantlib-{pin['version']}.data/data/share/doc/quantlib/LICENSE.TXT"
            if name == license_member:
                installed = Path(sys.prefix) / 'share/doc/quantlib/LICENSE.TXT'
            else:
                require(not any(part.endswith('.data') for part in Path(name).parts),
                        'unsupported wheel data member')
                installed = Path(distribution.locate_file(name))
            require(installed.is_file(), f'missing installed publisher file: {name}')
            sha = digest(archive.read(name))
            require(digest(installed.read_bytes()) == sha, f'installed QuantLib bytes differ: {name}')
            files[name] = sha
    require('QuantLib/__init__.py' in files and 'QuantLib/QuantLib.py' in files
            and any(name.startswith('QuantLib/_QuantLib') and name.endswith('.so') for name in files),
            'missing verified QuantLib wrapper or native extension')
    module = importlib.util.find_spec('QuantLib')
    require(module is not None and module.origin is not None
            and Path(module.origin).resolve() == Path(distribution.locate_file('QuantLib/__init__.py')).resolve(),
            'import path differs from verified installed QuantLib')
    return dict(project='QuantLib', version=pin['version'], wheel=pin['wheel'],
                wheel_sha256=pin['sha256'], installed_files=files,
                python=sys.version, executable=sys.executable,
                system=platform.system(), machine=platform.machine())


def ql_case(ql, fixture, row):
    """Actual QL coupons/schedules/index forecasting and DiscountingSwapEngine.

    Reject conventions outside this fixed suite. No Convex/manual outputs are
    read, no index-name preset or averaged notional/reset is substituted.
    """
    terms = row['terms']
    floating = terms['floating']
    valuation = ql.DateParser.parseISO(row['valuation_date'])
    qdate = ql.DateParser.parseISO
    iso = lambda date: date.ISO()
    dc = ql.Actual360()
    calendars = {}
    for name, holidays in fixture['calendar_holidays'].items():
        calendar = ql.BespokeCalendar(f"CVX019/{row['id']}/{name}/1")
        calendar.addWeekend(ql.Saturday)
        calendar.addWeekend(ql.Sunday)
        for date in holidays:
            calendar.addHoliday(qdate(date))
        calendars[name] = calendar

    def calendar(pin):
        require(pin['version'] == 1, 'unsupported calendar revision')
        return calendars[pin['id']]

    def profile(levels, start, end):
        eligible = [p for p in levels if qdate(p['start']) <= start and qdate(p['end']) >= end]
        require(len(eligible) == 1, 'comparator requires coupon-boundary profiles; no intracoupon approximation')
        return float(decimal(eligible[0]['value']))

    curves = {}
    recipes = fixture['curve_sets'][row['curve_set']]
    for recipe in recipes:
        require(recipe['time_basis'] == 'Act365Fixed'
                and recipe['representation'] == {'ZeroRates': {'compounding': 'Continuous'}}
                and recipe['interpolation'] == 'Linear' and recipe['extrapolation'] == 'None'
                and recipe['source'].keys() == {'Nodes'} and not recipe['dependencies'], 'unsupported comparator curve')
        nodes = recipe['source']['Nodes']
        curve = ql.ZeroCurve([qdate(n['pillar']['Date']) for n in nodes],
                             [n['quote']['value'] for n in nodes],
                             ql.Actual365Fixed(), ql.NullCalendar(), ql.Linear(), ql.Continuous, ql.Annual)
        require(iso(curve.referenceDate()) == recipe['reference_date'], 'curve reference date mismatch')
        curves[(recipe['key']['id'], recipe['key']['version'])] = curve

    def curve(key):
        return curves[(key['id'], key['version'])]

    projection = ql.YieldTermStructureHandle(curve(floating['projection_curve']))
    discount = curve(terms['collateral']['discount_curve'])
    if floating['kind'] == 'term':
        reset = floating['resets']
        require(reset['rate_period'] == 'IndexTenor' and reset['tenor_months'] == 3
                and reset['fixing_offset'] == {'BusinessDays': -2}
                and reset['reset_adjustment'] == 'Unadjusted'
                and reset['index_end_adjustment'] == 'Unadjusted' and not reset['index_end_of_month']
                and fixture['calendar_holidays'][reset['reset_calendar']['id']]
                == fixture['calendar_holidays'][reset['fixing_calendar']['id']], 'unsupported term-index mapping')
        index_id = reset['index_id']
        index = ql.IborIndex(f"{index_id}/{row['id']}", ql.Period(3, ql.Months), 2,
                             ql.USDCurrency(), calendar(reset['fixing_calendar']), ql.Unadjusted, False, dc, projection)
        pricer = ql.BlackIborCouponPricer(useIndexedCoupon=True)
    else:
        require(floating['kind'] == 'overnight', 'unsupported floating kind')
        index_id = floating['index_id']
        index = ql.OvernightIndex(f"{index_id}/{row['id']}", 0, ql.USDCurrency(),
                                  calendar(floating['observation_calendar']), dc, projection)
        pricer = ql.CompoundingOvernightIndexedCouponPricer()

    history = {}
    for observation in fixture['fixing_sets'][row['fixing_set']]:
        if observation['index_id'] != index_id or observation['known_at'] > row['known_at']:
            continue
        require(observation['status'] == 'Final' and observation['revision'] == 1
                and observation['published_at'] <= row['known_at'], 'ineligible declared fixing')
        date = qdate(observation['observation_date'])
        require(date <= valuation, 'future history is not admitted to this QL run')
        require(observation['observation_date'] not in history, 'duplicate eligible fixing')
        history[observation['observation_date']] = observation
        index.addFixing(date, float(decimal(observation['rate'])))

    include = row['on_payment'] == 'Include'
    require(include, 'this fixed comparator suite admits Include payment-date state')
    settings = ql.Settings.instance()
    previous = (settings.evaluationDate, settings.includeReferenceDateEvents, settings.enforcesTodaysHistoricFixings)
    settings.evaluationDate = valuation
    settings.includeReferenceDateEvents = include
    settings.enforcesTodaysHistoricFixings = True
    try:
        coupon_legs = []
        rates = {}
        spread_leg = []
        for leg_number, leg_terms in enumerate([terms['fixed']['leg'], floating['leg']]):
            spec = leg_terms['schedule']
            require(spec['frequency'] == 'Quarterly' and spec['stub_type'] == 'None'
                    and not spec['end_of_month'] and spec['first_regular_date'] is None
                    and spec['penultimate_date'] is None and leg_terms['day_count'] == 'Act360'
                    and leg_terms['rounding'] == 'Unrounded' and leg_terms['negative_coupons'] == 'Allow'
                    and leg_terms['dates']['accrual_adjustment'] == 'Unadjusted'
                    and leg_terms['dates']['payment_adjustment'] == 'Following'
                    and leg_terms['dates']['payment_delay'] == {'BusinessDays': 2}, 'unsupported coupon convention')
            schedule = ql.Schedule(qdate(spec['start_date']), qdate(spec['end_date']), ql.Period(3, ql.Months),
                                   calendar(leg_terms['calendars']['accrual']), ql.Unadjusted, ql.Unadjusted,
                                   ql.DateGeneration.Forward, False)
            dates = list(schedule)
            coupons = []
            for start, end in zip(dates, dates[1:]):
                payment = calendar(leg_terms['calendars']['payment']).advance(end, 2, ql.Days, ql.Following)
                if payment < valuation:
                    continue
                nominal = profile(leg_terms['notionals'], start, end)
                if leg_number == 0:
                    coupon = ql.FixedRateCoupon(payment, nominal, profile(terms['fixed']['rates'], start, end), dc, start, end)
                else:
                    spread = profile(floating['profiles']['spreads'], start, end)
                    gearing = profile(floating['profiles']['gearings'], start, end)
                    if floating['kind'] == 'term':
                        coupon = ql.IborCoupon(payment, nominal, start, end, 2, index,
                                               gearing=gearing, spread=spread, dayCounter=dc)
                        fixing_dates = [coupon.fixingDate()]
                        require(index.valueDate(fixing_dates[0]) == start, 'reset/index value date mismatch')
                    else:
                        convention = floating['convention']
                        coupon = ql.OvernightIndexedCoupon(payment, nominal, start, end, index,
                                                           gearing=gearing, spread=spread, dayCounter=dc,
                                                           telescopicValueDates=False, averagingMethod=ql.RateAveraging.Compound,
                                                           lookbackDays=convention['lookback_business_days'],
                                                           lockoutDays=convention['lockout_business_days'],
                                                           applyObservationShift=convention['observation_shift'])
                        fixing_dates = list(coupon.fixingDates())
                    coupon.setPricer(pricer)
                    # Force all real QL coupon observations before collecting lineage.
                    coupon.amount()
                    for date in fixing_dates:
                        observation = iso(date)
                        require(date > valuation or observation in history, f'missing due fixing: {index_id}/{observation}')
                        value_date = index.valueDate(date)
                        rates[observation] = dict(index=index_id, observation=observation,
                                                  rate_start=iso(value_date), rate_end=iso(index.maturityDate(value_date)),
                                                  rate=number(index.fixing(date)),
                                                  origin='Published' if observation in history else 'Projected',
                                                  revision=1 if observation in history else None)
                    spread_leg.append(ql.FixedRateCoupon(payment, nominal, spread, dc, start, end))
                coupons.append(coupon)
            coupon_legs.append(coupons)

        pay_fixed = terms['side'] == 'PayFixed'
        sign = -1 if pay_fixed else 1
        engine = ql.DiscountingSwapEngine(ql.YieldTermStructureHandle(discount), include, valuation, valuation)
        swap = ql.Swap(coupon_legs, [pay_fixed, not pay_fixed])
        swap.setPricingEngine(engine)
        fixed_pv, floating_pv = swap.legNPV(0), swap.legNPV(1)
        fixed_annuity, floating_annuity = abs(swap.legBPS(0)) * 10000, abs(swap.legBPS(1)) * 10000
        require(fixed_annuity > 0 and floating_annuity > 0, 'missing positive par exposure')
        spread_swap = ql.Swap([spread_leg], [False])
        spread_swap.setPricingEngine(engine)
        spread_pv = spread_swap.legNPV(0)
        unsigned_fixed, unsigned_float = fixed_pv * sign, floating_pv * -sign
        output = dict(id=row['id'], currency=terms['currency'], valuation_date=row['valuation_date'],
                      known_at=row['known_at'], side=terms['side'], pv=number(swap.NPV()),
                      fixed_pv=number(fixed_pv), floating_pv=number(floating_pv),
                      fixed_accrued=number(sign * sum(c.accruedAmount(valuation) for c in coupon_legs[0])),
                      floating_accrued=number(-sign * sum(c.accruedAmount(valuation) for c in coupon_legs[1])),
                      par=dict(fixed_rate=number(unsigned_float / fixed_annuity),
                               floating_spread=number((unsigned_fixed - unsigned_float + spread_pv) / floating_annuity),
                               fixed_rate_shift=number((unsigned_float - unsigned_fixed) / fixed_annuity),
                               floating_spread_shift=number((unsigned_fixed - unsigned_float) / floating_annuity),
                               fixed_annuity=number(fixed_annuity), floating_annuity=number(floating_annuity)),
                      rates=[rates[key] for key in sorted(rates)])
        for name, coupons, direction in [('fixed', coupon_legs[0], sign), ('floating', coupon_legs[1], -sign)]:
            output[name] = [dict(start=iso(c.accrualStartDate()), end=iso(c.accrualEndDate()), payment=iso(c.date()),
                                 year_fraction=number(c.accrualPeriod()), amount=number(direction * c.amount()),
                                 discount_ratio=number(discount.discount(c.date()) / discount.discount(valuation)),
                                 pv=number(direction * c.amount() * discount.discount(c.date()) / discount.discount(valuation)))
                            for c in coupons]
        return output
    finally:
        settings.evaluationDate, settings.includeReferenceDateEvents, settings.enforcesTodaysHistoricFixings = previous
        index.clearFixings()


def capture(wheel):
    fixture = inputs()
    runtime = verified_runtime(wheel, fixture)
    import QuantLib as ql  # only after all publisher/installed bytes have matched
    require(ql.__version__ == fixture['comparator']['version'], 'native QuantLib version mismatch')
    started = dt.datetime.now(dt.UTC).isoformat()
    cases = [ql_case(ql, fixture, row) for row in fixture['cases']]
    return dict(schema_version=1, engine='QuantLib', qualification='executed_quantlib_candidate',
                inputs=fixture, cases=cases, runtime=runtime, input_sha256=INPUT_SHA256,
                runner_sha256=digest((ROOT / SCRIPT).read_bytes()),
                started_at=started, finished_at=dt.datetime.now(dt.UTC).isoformat(), certified=False)


def compare(native, reference):
    """Numerical/schema comparison only; caller must verify execution evidence."""
    fixture = inputs()
    require(native['engine'] == 'Convex' and reference['engine'] == 'QuantLib', 'wrong output engine')
    require(native['schema_version'] == reference['schema_version'] == 1, 'wrong output schema')
    require(native['inputs'] == reference['inputs'] == fixture, 'output/input binding mismatch')
    require(tuple(c['id'] for c in native['cases']) == tuple(c['id'] for c in reference['cases']) == CASE_IDS,
            'missing/duplicate/reordered comparison case')
    errors = []
    compared = 0
    maxima = {key: Decimal(0) for key in BOUNDS if key != 'relative'}

    def close(actual, expected, bound, at):
        nonlocal compared
        difference = abs(decimal(actual) - decimal(expected))
        compared += 1
        maxima[bound] = max(maxima[bound], difference)
        if difference > Decimal(BOUNDS[bound]):
            errors.append(dict(field=at, actual=actual, reference=expected, absolute_error=str(difference), bound=BOUNDS[bound]))

    case_keys = {'id', 'currency', 'valuation_date', 'known_at', 'side', 'pv', 'fixed_pv', 'floating_pv',
                 'fixed_accrued', 'floating_accrued', 'par', 'fixed', 'floating', 'rates'}
    coupon_keys = {'start', 'end', 'payment', 'year_fraction', 'amount', 'discount_ratio', 'pv'}
    par_keys = {'fixed_rate', 'floating_spread', 'fixed_rate_shift', 'floating_spread_shift', 'fixed_annuity', 'floating_annuity'}
    rate_keys = {'index', 'observation', 'rate_start', 'rate_end', 'rate', 'origin', 'revision'}
    for actual, expected, source in zip(native['cases'], reference['cases'], fixture['cases'], strict=True):
        identity = source['id']
        require(actual.keys() == expected.keys() == case_keys, f'{identity}: missing/extra output field')
        for key, value in [('currency', source['terms']['currency']), ('side', source['terms']['side']),
                           ('valuation_date', source['valuation_date']), ('known_at', source['known_at'])]:
            require(actual[key] == expected[key] == value, f'{identity}/{key}: mismatched economics')
        for key in ['pv', 'fixed_pv', 'floating_pv', 'fixed_accrued', 'floating_accrued']:
            close(actual[key], expected[key], 'currency_abs', f'{identity}/{key}')
        require(actual['par'].keys() == expected['par'].keys() == par_keys, f'{identity}: missing par field')
        for key in par_keys:
            close(actual['par'][key], expected['par'][key], 'currency_abs' if key.endswith('annuity') else 'rate_abs', f'{identity}/par/{key}')
        for leg in ['fixed', 'floating']:
            require(0 < len(actual[leg]) == len(expected[leg]) <= 16, f'{identity}/{leg}: coupon count mismatch')
            for n, (a, e) in enumerate(zip(actual[leg], expected[leg], strict=True)):
                require(a.keys() == e.keys() == coupon_keys, f'{identity}/{leg}: coupon fields mismatch')
                require(all(a[k] == e[k] for k in ['start', 'end', 'payment']), f'{identity}/{leg}/{n}: date mismatch')
                for key, bound in [('year_fraction', 'year_fraction_abs'), ('amount', 'currency_abs'),
                                   ('discount_ratio', 'discount_ratio_abs'), ('pv', 'currency_abs')]:
                    close(a[key], e[key], bound, f'{identity}/{leg}/{n}/{key}')
        require(0 < len(actual['rates']) == len(expected['rates']) <= 4096, f'{identity}: observation count mismatch')
        seen = set()
        for a, e in zip(actual['rates'], expected['rates'], strict=True):
            require(a.keys() == e.keys() == rate_keys, f'{identity}: observation fields mismatch')
            require(all(a[k] == e[k] for k in rate_keys - {'rate'}), f'{identity}: observation/quotation/origin mismatch')
            key = (a['index'], a['observation'])
            require(key not in seen, f'{identity}: duplicate consumed observation')
            seen.add(key)
            close(a['rate'], e['rate'], 'rate_abs', f'{identity}/rates/{key}')
    return dict(status='matched_numerics' if not errors else 'numerical_mismatch', cases=len(CASE_IDS),
                compared_values=compared, maxima={k: str(v) for k, v in maxima.items()},
                tolerances=BOUNDS, errors=errors, certified=False)


def source_pins():
    library = loads((ROOT / 'tools/library/library-inventory.json').read_bytes())
    inventory = loads((ROOT / 'tools/assurance/inventory.json').read_bytes())
    paths = set(library['inspected_sources']) | {r['path'] for r in inventory['files']}
    paths |= {SCRIPT, EXPORT, INPUT.relative_to(ROOT).as_posix(), 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'}
    paths |= {p.relative_to(ROOT).as_posix() for p in (ROOT / 'crates').glob('*/Cargo.toml')}
    return {path: digest((ROOT / path).read_bytes()) for path in sorted(paths)}


def recorded(command, directory, name):
    started = dt.datetime.now(dt.UTC).isoformat()
    clock = time.monotonic()
    error = None
    try:
        process = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=300)
        stdout, stderr, code = process.stdout, process.stderr, process.returncode
    except subprocess.TimeoutExpired as failure:
        stdout, stderr, code = failure.stdout or b'', failure.stderr or b'', None
        error = 'execution_timeout'
    except OSError as failure:
        stdout, stderr, code = b'', str(failure).encode(), None
        error = 'execution_not_started'
    (directory / f'{name}.stdout').write_bytes(stdout)
    (directory / f'{name}.stderr').write_bytes(stderr)
    return dict(name=name, command=command, started_at=started, finished_at=dt.datetime.now(dt.UTC).isoformat(),
                duration_seconds=round(time.monotonic() - clock, 3), exit_code=code, error=error,
                stdout=f'{name}.stdout', stderr=f'{name}.stderr')


def run(python, wheel, directory):
    fixture = inputs()
    directory.mkdir(parents=True, exist_ok=False)  # each attempt preserves its own raw evidence
    (directory / 'inputs.json').write_bytes(INPUT.read_bytes())
    pins = source_pins()
    commands = []
    build = None
    report = dict(status='execution_incomplete', certified=False, accepted_task=False)
    try:
        commands.append(recorded([str(python), str(ROOT / SCRIPT), 'capture', '--wheel', str(wheel)], directory, 'quantlib'))
        require(commands[-1]['exit_code'] == 0, 'QuantLib capture failed; see quantlib.stderr')
        commands.append(recorded(['rustc', '-Vv'], directory, 'rustc'))
        require(commands[-1]['exit_code'] == 0, 'Rust build identity unavailable')
        commands.append(recorded(['cargo', '--version'], directory, 'cargo-version'))
        require(commands[-1]['exit_code'] == 0, 'Cargo build identity unavailable')
        commands.append(recorded(['cargo', 'build', '--locked', '--message-format=json', '-p', 'convex',
                                  '--example', 'swap_comparison'], directory, 'build'))
        require(commands[-1]['exit_code'] == 0, 'native build failed; see build.stderr')
        messages = [loads(line) for line in (directory / 'build.stdout').read_bytes().splitlines()]
        candidates = [m for m in messages if m.get('reason') == 'compiler-artifact'
                      and m['target']['name'] == 'swap_comparison' and m['target']['kind'] == ['example']
                      and m.get('executable')]
        require(len(candidates) == 1, 'missing unambiguous Cargo-built candidate executable')
        binary = Path(candidates[0]['executable']).resolve()
        build = dict(executable=str(binary), sha256=digest(binary.read_bytes()),
                     profile=candidates[0]['profile'], features=candidates[0]['features'],
                     environment={key: os.environ.get(key) for key in ['CARGO_TARGET_DIR', 'RUSTFLAGS', 'RUSTDOCFLAGS']})
        commands.append(recorded([str(binary)], directory, 'native'))
        require(commands[-1]['exit_code'] == 0, 'native export failed; see native.stderr')
        require(source_pins() == pins, 'source changed during comparison execution')
        native = loads((directory / 'native.stdout').read_bytes())
        reference = loads((directory / 'quantlib.stdout').read_bytes())
        report = compare(native, reference)
        report.update(qualification='executed_engineering_comparison', accepted_task=False)
    except (Failure, OSError, subprocess.TimeoutExpired) as error:
        report['error'] = str(error)
    def preserve():
        write(directory / 'commands.json', commands)
        write(directory / 'report.json', report)
        write(directory / 'manifest.json', dict(schema_version=1, input_sha256=INPUT_SHA256,
              runner_sha256=pins[SCRIPT], source_pins=pins, native_build=build, certified=False,
              artifact_sha256={p.name: digest(p.read_bytes()) for p in sorted(directory.iterdir())
                              if p.is_file() and p.name != 'manifest.json'}))

    preserve()
    if report['status'] == 'matched_numerics':
        try:
            replay(directory)  # numerical equality alone cannot approve incomplete execution provenance
        except (Failure, OSError, KeyError, TypeError, RuntimeError) as error:
            report = dict(status='execution_incomplete', error=str(error), certified=False, accepted_task=False)
            preserve()
    require(fixture['certified'] is False, 'input certificate drift')
    return report


def replay(directory):
    """Replay a run against its exact current calculation/protocol sources."""
    return replay_execution(directory, current_sources=True)


def replay_execution(directory, *, current_sources):
    manifest = loads((directory / 'manifest.json').read_bytes())
    require(manifest['schema_version'] == 1 and manifest['certified'] is False
            and manifest['input_sha256'] == INPUT_SHA256
            and manifest['runner_sha256'] == manifest['source_pins'][SCRIPT], 'capture source/protocol mismatch')
    if current_sources:
        require(manifest['source_pins'] == source_pins()
                and manifest['runner_sha256'] == digest((ROOT / SCRIPT).read_bytes()), 'capture source/protocol mismatch')
    artifacts = {p.name: digest(p.read_bytes()) for p in directory.iterdir() if p.is_file() and p.name != 'manifest.json'}
    require(artifacts == manifest['artifact_sha256'], 'capture artifact bytes/set changed')
    inputs((directory / 'inputs.json').read_bytes())
    commands = loads((directory / 'commands.json').read_bytes())
    require([c['name'] for c in commands] == ['quantlib', 'rustc', 'cargo-version', 'build', 'native']
            and all(c['exit_code'] == 0 and c['error'] is None and c['stdout'] == c['name'] + '.stdout'
                    and c['stderr'] == c['name'] + '.stderr' for c in commands),
            'missing successful actual engine executions')
    for command in commands:
        require(command['duration_seconds'] >= 0
                and dt.datetime.fromisoformat(command['finished_at']) >= dt.datetime.fromisoformat(command['started_at']),
                'invalid command timestamps')
    require(commands[1]['command'] == ['rustc', '-Vv']
            and commands[2]['command'] == ['cargo', '--version']
            and commands[3]['command'] == ['cargo', 'build', '--locked', '--message-format=json', '-p', 'convex',
                                           '--example', 'swap_comparison']
            and commands[0]['command'][-3:-1] == ['capture', '--wheel']
            and Path(commands[0]['command'][-1]).name == inputs()['comparator']['wheel'],
            'unexpected engine/build commands')
    reference = loads((directory / 'quantlib.stdout').read_bytes())
    runtime = reference['runtime']
    pin = inputs()['comparator']
    files = runtime['installed_files']
    require('QuantLib/__init__.py' in files and 'QuantLib/QuantLib.py' in files
            and any(name.startswith('QuantLib/_QuantLib') and name.endswith('.so') for name in files)
            and all(isinstance(value, str) and len(value) == 64 and all(c in '0123456789abcdef' for c in value)
                    for value in files.values()), 'incomplete verified installed-runtime payloads')
    require(reference['qualification'] == 'executed_quantlib_candidate' and reference['certified'] is False
            and reference['input_sha256'] == INPUT_SHA256 and reference['runner_sha256'] == manifest['runner_sha256']
            and runtime['version'] == pin['version'] and runtime['wheel'] == pin['wheel']
            and runtime['wheel_sha256'] == pin['sha256'] and runtime['system'] == 'Darwin'
            and runtime['machine'] == 'arm64' and runtime['installed_files'], 'missing verified QuantLib provenance')
    require(dt.datetime.fromisoformat(reference['finished_at']) >= dt.datetime.fromisoformat(reference['started_at']), 'invalid capture timestamps')
    native = loads((directory / 'native.stdout').read_bytes())
    build = manifest['native_build']
    require(native['qualification'] == 'native_candidate_not_external_evidence'
            and isinstance(native['package_version'], str) and native['package_version']
            and build is not None and len(build['sha256']) == 64
            and commands[-1]['command'] == [build['executable']], 'native qualification/build mismatch')
    result = compare(native, reference)
    require(result['status'] == 'matched_numerics', 'saved comparison contains numerical mismatches')
    saved = loads((directory / 'report.json').read_bytes())
    require({k: saved[k] for k in result} == result and saved['accepted_task'] is False
            and saved['qualification'] == 'executed_engineering_comparison', 'saved report differs from replay')
    return dict(result, qualification='replayed_execution_evidence', accepted_task=False)


def frozen_reference():
    """Read only the admitted immutable capture, never an arbitrary golden path.

    Capture replay checks original execution/source identity and every capture
    byte against the capture manifest. A future native candidate may differ from
    that captured native revision; the independent QL outputs stay fixed.
    """
    replay_execution(REFERENCE, current_sources=False)
    return loads((REFERENCE / 'quantlib.stdout').read_bytes())


def offline(binary, report_path):
    """Execute today's candidate against the fixed independently captured QL."""
    reference = frozen_reference()
    require(binary.is_file(), 'missing native candidate executable')
    require(not report_path.is_relative_to(REFERENCE), 'report cannot overwrite immutable reference')
    report_path.parent.mkdir(parents=True, exist_ok=True)
    binary_sha = digest(binary.read_bytes())
    pins = source_pins()
    command = recorded([str(binary)], report_path.parent, report_path.stem + '-native')
    require(command['exit_code'] == 0, 'native offline candidate failed')
    require(digest(binary.read_bytes()) == binary_sha and source_pins() == pins, 'offline candidate/source changed during execution')
    native = loads((report_path.parent / command['stdout']).read_bytes())
    require(native['qualification'] == 'native_candidate_not_external_evidence', 'native qualification mismatch')
    result = compare(native, reference)
    result.update(qualification='current_native_against_frozen_quantlib_execution', accepted_task=False,
                  input_sha256=INPUT_SHA256, reference_sha256=digest((REFERENCE / 'quantlib.stdout').read_bytes()),
                  native_executable_sha256=binary_sha, command=command, source_pins=pins,
                  stdout_sha256=digest((report_path.parent / command['stdout']).read_bytes()),
                  stderr_sha256=digest((report_path.parent / command['stderr']).read_bytes()))
    write(report_path, result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='mode', required=True)
    commands.add_parser('check', help='check pinned input scope/bounds; no runtime or external evidence')
    capture_parser = commands.add_parser('capture', help='execute verified QL coupons in this Python; used by run')
    capture_parser.add_argument('--wheel', type=Path, required=True)
    run_parser = commands.add_parser('run', help='execute both engines and save a new evidence directory')
    run_parser.add_argument('--quantlib-python', type=Path, required=True)
    run_parser.add_argument('--wheel', type=Path, required=True)
    run_parser.add_argument('--output', type=Path, required=True)
    replay_parser = commands.add_parser('replay', help='verify a successful saved execution and compare offline')
    replay_parser.add_argument('directory', type=Path)
    offline_parser = commands.add_parser('offline', help='run a current native binary against the pinned captured QL reference')
    offline_parser.add_argument('--candidate-binary', type=Path, required=True)
    offline_parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.mode == 'check':
            fixture = inputs()
            result = dict(status='pinned_inputs_validated', cases=len(CASE_IDS), input_sha256=INPUT_SHA256,
                          tolerances=fixture['tolerances'], external_execution='required', certified=False)
        elif args.mode == 'capture':
            result = capture(args.wheel.resolve())
        elif args.mode == 'run':
            # Resolving a venv interpreter symlink selects the base interpreter
            # and loses its installed packages. Preserve the supplied entry point.
            result = run(args.quantlib_python.absolute(), args.wheel.resolve(), args.output.resolve())
        elif args.mode == 'replay':
            result = replay(args.directory.resolve())
        else:
            result = offline(args.candidate_binary.resolve(), args.report.resolve())
        print(json.dumps(result, allow_nan=False))
        return 0 if result.get('status') != 'execution_incomplete' and result.get('status') != 'numerical_mismatch' else 1
    except (Failure, OSError, KeyError, TypeError, RuntimeError, importlib.metadata.PackageNotFoundError,
            subprocess.SubprocessError, zipfile.BadZipFile) as error:
        print(json.dumps(dict(status='failed', error=str(error), certified=False)), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
