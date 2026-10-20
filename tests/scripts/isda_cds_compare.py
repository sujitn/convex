#!/usr/bin/env python3
"""Offline original ISDA RFR grid admission and actual native CDS comparison.

Python 3.11+, standard library only. Original model execution is frozen with
source/compiler/command provenance; no ISDA binary or network is needed in CI.
This is scoped engineering evidence, never an independent market certificate.
"""
from __future__ import annotations
import argparse
import datetime as dt
from decimal import Decimal, InvalidOperation
from io import BytesIO
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PACK = Path('tests/fixtures/comparisons/cds-isda-1.8.3')
PINS = {
    'NYM.csv': '063a3d24d8772d06281fc6252d40e0d4f5157e0c6ac8c3448eece13c458d2c4f',
    'USD_SOFR_20220621.zip': '4ecf867697c1a6b4e0966743727820bf8b81f9db60e0e4fe55c1c804f4b981da',
    'compiler.stdout': 'f6a41663474c412322e565ec1a1aa27bcb42151b98d527dc401b7d744b9a4237',
    'grid.json': '4454bc12f037d67a5e7a2061ad168b11c761fa3a41991ac315f6fc0a8622c7ef',
    'inputs.json': '3d850ab14c7bf2e8c7aea98f16129e7a2a24ee3bc2b8ebce3458ee20f40704d2',
    'isda-commands.json': 'e9dea5d4c8e54781efe8b4e4661053e525e376f80328d35525be21a665f6b288',
    'isda-manifest.json': '2f48ad9ddf544166c631f77cece997a879845d67334a9c00b836a6a4dce6a118',
    'isda-reference.json': 'abe06a7056dfbceb169718ae2d01aea5e08839b1ce0131aa7e6fbe5e3504b35b',
    'isda-reference.stderr': 'e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855',
}
DRIVER = 'tests/scripts/isda_cds_capture.c'
DRIVER_SHA = 'c21c9d6af1c672f06057cdf1af7e94e223c9d9d5972cd1b20f2ebcbc512f4064'
LICENSE = 'licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt'
LICENSE_SHA = 'fb51cb1bf23e2df84fad1d65fd45f3726335c0aa9c0c0e7fd6af7660126aeb28'
SOURCE_SHA = 'bee02772ef45c3ff93de995a1655b3c5231aec4e257173bc4d21d47c362e0db0'
NOTICE = ('This application is based on the ISDA CDS Standard Model (version 1.8.3), '
          'developed and supported in collaboration with Markit Group Ltd.')
BOUNDS = {
    'clean_upfront_fraction': {'absolute': '1e-8', 'relative': '0', 'unit': 'fraction_of_notional'},
    'cash_settlement_amount': {'absolute': '0.5', 'relative': '0', 'unit': 'USD'},
    'clean_price_per100': {'absolute': '1e-6', 'relative': '0', 'unit': 'per_100'},
    'accrued_days': {'absolute': '1e-8', 'relative': '0', 'unit': 'calendar_days'},
    'accrued_premium': {'absolute': '1e-6', 'relative': '0', 'unit': 'USD'},
}
INPUT_FIELDS = ('row', 'trade_date', 'maturity_date', 'currency', 'coupon_bps', 'spread_bps',
                'recovery', 'cash_settlement_date', 'step_in_date')
EXTRA_NUMBERS = ('dirty_upfront_fraction', 'hazard_per_act365f_year', 'native_quote_residual', 'iterations')
NS = {'s': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main'}


class Failure(ValueError):
    """An explicit rejection, including with Python assertions disabled."""


def require(condition, message):
    if not condition:
        raise Failure(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unique(pairs):
    out = {}
    for key, value in pairs:
        require(key not in out, f'duplicate JSON key: {key}')
        out[key] = value
    return out


def loads(data):
    require(len(data) <= 8 * 1024 * 1024, 'comparison JSON exceeds work bound')
    def reject(value):
        raise Failure(f'nonfinite JSON: {value}')
    return json.loads(data, object_pairs_hook=unique, parse_constant=reject)


def number(value):
    require(type(value) in (str, int, float) and len(str(value)) <= 96, 'invalid numeric output')
    try:
        result = Decimal(str(value))
    except InvalidOperation as error:
        raise Failure('invalid decimal output') from error
    require(result.is_finite() and abs(result) <= Decimal('1e12'), 'nonfinite or excessive output')
    return result


def published_cells(root):
    """Read original OOXML values verbatim: no formula evaluation or workbook edit."""
    with zipfile.ZipFile(root / PACK / 'USD_SOFR_20220621.zip') as archive:
        raw = archive.read('USD_SOFR_20220621.xlsx')
    require(digest(raw) == '9c313474900c6c260f414ef0c29ec07147b892cc5f29f512743e4e7e760dbf42', 'workbook pin drift')
    with zipfile.ZipFile(BytesIO(raw)) as workbook:
        strings = [''.join(x.itertext()) for x in ET.fromstring(workbook.read('xl/sharedStrings.xml'))]
        def rows(name):
            for row in ET.fromstring(workbook.read(name)).find('s:sheetData', NS):
                cells = {}
                for cell in row:
                    value = cell.find('s:v', NS)
                    require(cell.find('s:f', NS) is None, 'formula-bearing workbook needs separate admission')
                    if value is not None:
                        cells[cell.attrib['r'].rstrip('0123456789')] = strings[int(value.text)] if cell.get('t') == 's' else value.text
                yield int(row.attrib['r']), cells
        def date(value):
            return (dt.date(1899, 12, 30) + dt.timedelta(days=int(value))).isoformat()
        cases = []
        for row, c in rows('xl/worksheets/sheet1.xml'):
            if row < 2 or not c:
                continue
            require(set(c) == set('ABCDEFGHIJKLM'), 'original grid column scope drift')
            cases.append(dict(row=row, trade_date=date(c['A']), maturity_date=date(c['B']), currency=c['C'].strip(),
                coupon_bps=c['D'], spread_bps=c['E'], recovery=c['F'], clean_upfront_fraction=c['G'],
                cash_settlement_amount=c['H'], clean_price_per100=c['I'], accrued_days=c['J'],
                accrued_premium=c['K'], cash_settlement_date=date(c['L']), step_in_date=date(c['M'])))
        quotes = [dict(row=r, instrument_type=c['A'], tenor=c['B'], decimal_rate=c['C'])
                  for r, c in rows('xl/worksheets/sheet2.xml') if r >= 3 and c]
    require(len(cases) == 7164 and len(quotes) == 19, 'published scope drift')
    return cases, quotes


def compare(cases, grid):
    require(grid['comparison_bounds'] == BOUNDS, 'predeclared bounds changed')
    require(len(grid['cases']) == 7164 and [c.get('row') for c in grid['cases']] == list(range(2, 7166)), 'published case scope differs')
    require(len(cases) == 7164 and [c.get('row') for c in cases] == list(range(2, 7166)), 'missing, duplicate or reordered cases')
    maxima = {k: Decimal(0) for k in BOUNDS}
    errors = []
    for actual, expected in zip(cases, grid['cases']):
        for name, bound in BOUNDS.items():
            require(name in actual, f'missing measure {expected["row"]}/{name}')
            difference = abs(number(actual[name]) - number(expected[name]))
            maxima[name] = max(maxima[name], difference)
            if difference > Decimal(bound['absolute']):
                errors.append(dict(row=expected['row'], measure=name, actual=str(actual[name]),
                                   expected=expected[name], absolute_error=str(difference), bound=bound))
    return dict(status='passed' if not errors else 'numerical_mismatch', cases=7164, compared_values=35820,
                max_absolute_errors={k: str(v) for k, v in maxima.items()}, errors=errors, certified=False)


def reference(root=ROOT):
    for name, sha in PINS.items():
        require(digest((root / PACK / name).read_bytes()) == sha, f'original pin/bytes differ: {name}')
    require(digest((root / DRIVER).read_bytes()) == DRIVER_SHA, 'original capture driver changed')
    require(digest((root / LICENSE).read_bytes()) == LICENSE_SHA, 'original license bytes differ')
    grid = loads((root / PACK / 'grid.json').read_bytes())
    inputs = loads((root / PACK / 'inputs.json').read_bytes())
    captured = loads((root / PACK / 'isda-reference.json').read_bytes())
    manifest = loads((root / PACK / 'isda-manifest.json').read_bytes())
    commands = loads((root / PACK / 'isda-commands.json').read_bytes())
    cases, quotes = published_cells(root)
    require(grid['cases'] == cases and grid['rate_quotes'] == quotes, 'outputs differ from original workbook cells')
    require(inputs['cases'] == [{k: c[k] for k in INPUT_FIELDS} for c in cases], 'input/output scope differs')
    require(inputs['source_archive_sha256'] == grid['archive_sha256'] == PINS['USD_SOFR_20220621.zip']
            and inputs['reference_model_version'] == grid['model_version'] == '1.8.3', 'model/input provenance mismatch')
    require(manifest['executed'] is True and manifest['certified'] is False
            and manifest['source_sha256'] == SOURCE_SHA and manifest['driver_sha256'] == DRIVER_SHA
            and manifest['grid_sha256'] == PINS['grid.json'] and manifest['calendar_sha256'] == PINS['NYM.csv']
            and manifest['source_files']['license.txt'] == LICENSE_SHA, 'executed original model identity differs')
    require(captured['model_version'] == 'CDS version "1.8.3"' and captured['curve_base_days'] == 2, 'actual model version/spot anchor differs')
    require(len(commands) == 53 and commands[0]['label'] == 'compiler'
            and commands[-1]['label'] == 'execution', 'original build/execution command chain missing')
    previous = None
    for command in commands:
        start, finish = (dt.datetime.fromisoformat(command[k]) for k in ('started_at', 'finished_at'))
        require(command['exit_code'] == 0 and command['argv'] and start.tzinfo is not None
                and finish >= start and (previous is None or start >= previous), 'failed/invalid original command provenance')
        previous = finish
    require(commands[0]['stdout_sha256'] == PINS['compiler.stdout']
            and commands[-1]['stdout_sha256'] == PINS['isda-reference.json']
            and commands[-1]['stderr_sha256'] == PINS['isda-reference.stderr'], 'raw output/execution binding differs')
    require(len(captured['nodes']) == 36, 'complete external discount knots omitted')
    valuation = dt.date.fromisoformat(inputs['template']['trade_date'])
    nodes = inputs['discount_recipe']['source']['Nodes']
    require(nodes == [dict(pillar={'Date': (valuation + dt.timedelta(days=n['days'])).isoformat()},
                           quote=dict(value=n['df'], unit='DiscountFactor')) for n in captured['nodes']],
            'native discount inputs differ from actual original model')
    require(inputs['calendar_holidays'] == [dt.datetime.strptime(d, '%Y%m%d').date().isoformat()
            for d in (root / PACK / 'NYM.csv').read_text().splitlines()], 'official calendar differs')
    result = compare(captured['cases'], grid)
    require(result['status'] == 'passed', 'original model does not reproduce published grid')
    return grid, inputs, result


def validate_candidate(candidate, grid, inputs):
    require(type(candidate) is dict and set(candidate) == {'schema_version', 'engine', 'model_version', 'inputs_sha256', 'notice', 'cases'}
            and candidate['schema_version'] == 1 and candidate['engine'] == 'Convex'
            and candidate['model_version'] == '1.8.3' and candidate['inputs_sha256'] == PINS['inputs.json']
            and candidate['notice'] == NOTICE, 'candidate model/input/notice protocol differs')
    require(type(candidate['cases']) is list and len(candidate['cases']) == 7164, 'candidate case count differs')
    for actual, source in zip(candidate['cases'], inputs['cases']):
        require(type(actual) is dict and set(actual) == set(INPUT_FIELDS) | set(BOUNDS) | set(EXTRA_NUMBERS), 'candidate fields differ')
        require({k: actual[k] for k in INPUT_FIELDS} == source, 'candidate dates/identities/economics differ')
        for name in EXTRA_NUMBERS:
            number(actual[name])
        require(0 <= number(actual['hazard_per_act365f_year']) <= 1
                and abs(number(actual['native_quote_residual'])) <= Decimal('1e-12')
                and type(actual['iterations']) is int and 0 <= actual['iterations'] <= 100,
                'conversion bracket/residual/iteration postcondition failed')
        clean, dirty, accrued = (number(actual[k]) for k in ('clean_upfront_fraction', 'dirty_upfront_fraction', 'accrued_premium'))
        require(abs(clean-dirty-accrued/Decimal('10000000')) <= Decimal('1e-12')
                and abs(number(actual['cash_settlement_amount'])-dirty*Decimal('10000000')) <= Decimal('1e-7'),
                'explicit clean/dirty/currency unit identity failed')
    return compare(candidate['cases'], grid)


def offline(binary, report):
    grid, inputs, original = reference()
    binary = binary.resolve(strict=True)
    report = report.absolute()
    report.parent.mkdir(parents=True, exist_ok=True)
    output = report.with_name(report.stem + '-native.stdout')
    errors = report.with_name(report.stem + '-native.stderr')
    require(not any(p.exists() for p in (report, output, errors)), 'refusing to overwrite a comparison run')
    result = dict(status='execution_incomplete', certified=False, original_model=original,
                  original_pins=PINS, binary=str(binary), binary_sha256=digest(binary.read_bytes()))
    result['source_pins'] = {str(p.relative_to(ROOT)): digest(p.read_bytes()) for p in
        [ROOT/'Cargo.toml', ROOT/'Cargo.lock', ROOT/'crates/convex-credit/Cargo.toml',
         *sorted((ROOT/'crates/convex-credit/src').glob('*.rs')),
         ROOT/'crates/convex-credit/examples/cds_comparison.rs', ROOT/'tests/scripts/isda_cds_compare.py']}
    start = dt.datetime.now(dt.timezone.utc).isoformat()
    try:
        with output.open('wb') as stdout, errors.open('wb') as stderr:
            run = subprocess.run([str(binary)], cwd=ROOT, stdout=stdout, stderr=stderr, timeout=60)
        result['command'] = dict(argv=[str(binary)], started_at=start,
            finished_at=dt.datetime.now(dt.timezone.utc).isoformat(), exit_code=run.returncode,
            stdout_sha256=digest(output.read_bytes()), stderr_sha256=digest(errors.read_bytes()))
        require(run.returncode == 0, 'actual native subprocess failed')
        result['native'] = validate_candidate(loads(output.read_bytes()), grid, inputs)
        result['status'] = result['native']['status']
        result['qualification'] = 'actual original published USD RFR grid and native CDS subset; no independent market/model/production certificate'
    except (Failure, ValueError, KeyError, OSError, subprocess.TimeoutExpired) as error:
        result['error'] = str(error)
    report.write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='mode', required=True)
    sub.add_parser('check')
    run = sub.add_parser('offline')
    run.add_argument('--candidate-binary', type=Path, required=True)
    run.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    try:
        result = reference()[2] if args.mode == 'check' else offline(args.candidate_binary, args.report)
        print(json.dumps(result))
        return 0 if result['status'] == 'passed' else 1
    except (Failure, ValueError, KeyError, OSError) as error:
        print(json.dumps(dict(status='rejected', error=str(error), certified=False)))
        return 1


if __name__ == '__main__':
    sys.exit(main())
