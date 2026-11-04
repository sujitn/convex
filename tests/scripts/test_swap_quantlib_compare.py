"""Fail-closed comparison controls. Synthetic outputs are never external evidence.

These tests exercise rejection/threshold/provenance mechanics without importing
or imitating QuantLib. Actual QuantLib execution remains a separate required run.
"""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock
import zipfile

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('swap_compare', Path(__file__).with_name('swap_quantlib_compare.py'))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


def synthetic_control_outputs():
    fixture = comparison.inputs()
    cases = []
    for source in fixture['cases']:
        terms = source['terms']
        floating = terms['floating']
        index = floating['resets']['index_id'] if floating['kind'] == 'term' else floating['index_id']
        coupon = dict(start='2024-04-02', end='2024-07-02', payment='2024-07-05',
                      year_fraction='0.2527777777777778', amount='-10', discount_ratio='1', pv='-10')
        cases.append(dict(id=source['id'], currency=terms['currency'], side=terms['side'],
                          valuation_date=source['valuation_date'], known_at=source['known_at'],
                          pv='1', fixed_pv='-10', floating_pv='11', fixed_accrued='-4', floating_accrued='5',
                          par=dict(fixed_rate='0.044', floating_spread='0.0005', fixed_rate_shift='0.003',
                                   floating_spread_shift='-0.0005', fixed_annuity='250000', floating_annuity='250000'),
                          fixed=[coupon], floating=[dict(coupon, amount='11', pv='11')],
                          rates=[dict(index=index, observation='2024-06-17', rate_start='2024-06-17',
                                      rate_end='2024-06-18', rate='0.038', origin='Published', revision=1)]))
    native = dict(schema_version=1, engine='Convex', inputs=fixture, cases=cases,
                  qualification='synthetic_control_only')
    reference = dict(copy.deepcopy(native), engine='QuantLib')
    return native, reference


class ComparisonControls(unittest.TestCase):
    def test_publisher_data_license_is_verified_after_install_relocation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            wheel = root / 'quantlib-1.40.whl'
            members = {'QuantLib/__init__.py': b'init', 'QuantLib/QuantLib.py': b'wrapper',
                       'QuantLib/_QuantLib.abi3.so': b'extension',
                       'quantlib-1.40.data/data/share/doc/quantlib/LICENSE.TXT': b'publisher license'}
            with zipfile.ZipFile(wheel, 'w') as archive:
                for name, data in members.items():
                    archive.writestr(name, data)
                    installed = root / ('share/doc/quantlib/LICENSE.TXT' if '.data/' in name else name)
                    installed.parent.mkdir(parents=True, exist_ok=True)
                    installed.write_bytes(data)
            fixture = {'comparator': {'wheel': wheel.name, 'sha256': comparison.digest(wheel.read_bytes()), 'version': '1.40'}}
            distribution = SimpleNamespace(version='1.40', locate_file=lambda name: root / name)
            module = SimpleNamespace(origin=str(root / 'QuantLib/__init__.py'))
            with mock.patch.object(comparison.platform, 'system', return_value='Darwin'), \
                    mock.patch.object(comparison.platform, 'machine', return_value='arm64'), \
                    mock.patch.object(comparison.importlib.metadata, 'distribution', return_value=distribution), \
                    mock.patch.object(comparison.importlib.util, 'find_spec', return_value=module), \
                    mock.patch.object(sys, 'prefix', str(root)):
                result = comparison.verified_runtime(wheel, fixture)
                self.assertEqual(set(result['installed_files']), set(members))
                (root / 'share/doc/quantlib/LICENSE.TXT').write_bytes(b'changed license')
                with self.assertRaisesRegex(comparison.Failure, 'installed QuantLib bytes differ'):
                    comparison.verified_runtime(wheel, fixture)

    def test_frozen_reference_is_a_real_complete_capture_and_not_a_certificate(self):
        reference = comparison.frozen_reference()
        self.assertEqual(reference['qualification'], 'executed_quantlib_candidate')
        self.assertEqual(len(reference['cases']), 10)
        self.assertFalse(reference['certified'])
        self.assertEqual(reference['runtime']['wheel_sha256'], comparison.inputs()['comparator']['sha256'])

    def test_frozen_reference_rejects_changed_raw_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            reference = root / comparison.REFERENCE.relative_to(comparison.ROOT)
            shutil.copytree(comparison.REFERENCE, reference)
            (reference / 'quantlib.stdout').write_bytes(b'{}')
            with mock.patch.object(comparison, 'ROOT', root), mock.patch.object(comparison, 'REFERENCE', reference):
                with self.assertRaisesRegex(comparison.Failure, 'capture artifact bytes/set changed'):
                    comparison.frozen_reference()

    def test_generic_replay_still_requires_the_actual_current_source_identity(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / 'capture'
            shutil.copytree(comparison.REFERENCE, directory)
            manifest = json.loads((directory / 'manifest.json').read_text())
            manifest['runner_sha256'] = manifest['source_pins'][comparison.SCRIPT] = '0' * 64
            comparison.write(directory / 'manifest.json', manifest)
            with self.assertRaisesRegex(comparison.Failure, 'capture source/protocol mismatch'):
                comparison.replay(directory)

    def test_cli_preserves_supplied_virtual_environment_interpreter(self):
        with tempfile.TemporaryDirectory() as temporary:
            interpreter = Path(temporary) / 'venv-python'
            interpreter.symlink_to(Path(sys.executable).resolve())
            arguments = ['compare', 'run', '--quantlib-python', str(interpreter),
                         '--wheel', str(Path(temporary) / 'runtime.whl'),
                         '--output', str(Path(temporary) / 'capture')]
            with mock.patch.object(sys, 'argv', arguments), \
                    mock.patch.object(comparison, 'run', return_value={'status': 'execution_incomplete'}) as execute, \
                    mock.patch('builtins.print'):
                self.assertEqual(comparison.main(), 1)
            self.assertEqual(execute.call_args.args[0], interpreter.absolute())
            self.assertNotEqual(execute.call_args.args[0], interpreter.resolve())

    def test_pinned_inputs_contain_all_declared_contracts_and_no_external_outputs(self):
        fixture = comparison.inputs()
        self.assertFalse(fixture['certified'])
        self.assertEqual(tuple(c['id'] for c in fixture['cases']), comparison.CASE_IDS)
        self.assertNotIn('expected', fixture)
        self.assertEqual(fixture['tolerances']['relative'], '0')

    def test_input_mutation_is_rejected_before_execution(self):
        raw = comparison.INPUT.read_bytes().replace(b'"1000000"', b'"1000001"', 1)
        with self.assertRaisesRegex(comparison.Failure, 'inputs changed'):
            comparison.inputs(raw)

    def test_duplicate_keys_nonfinite_and_large_json_are_rejected(self):
        for raw in [b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b' ' * (4 * 1024 * 1024 + 1)]:
            with self.assertRaises(comparison.Failure):
                comparison.loads(raw)

    def test_decimal_validation_includes_negative_rates_and_rejects_nonfinite_values(self):
        self.assertEqual(comparison.decimal('-0.003'), comparison.Decimal('-0.003'))
        for value in ['NaN', 'Infinity', '-Infinity', '1e20', 'bad', 0.1, True, None, '']:
            with self.assertRaises(comparison.Failure):
                comparison.decimal(value)

    def test_currency_absolute_bound_is_inclusive_and_never_relative(self):
        native, reference = synthetic_control_outputs()
        native['cases'][0]['pv'] = '1.0000001'
        self.assertEqual(comparison.compare(native, reference)['status'], 'matched_numerics')
        native['cases'][0]['pv'] = '1.00000010000001'
        result = comparison.compare(native, reference)
        self.assertEqual(result['status'], 'numerical_mismatch')
        self.assertEqual(result['errors'][0]['field'], 'irs-seasoned/pv')
        self.assertFalse(result['certified'])

    def test_par_rate_and_discount_ratio_use_their_separate_predeclared_bounds(self):
        native, reference = synthetic_control_outputs()
        native['cases'][0]['par']['fixed_rate'] = '0.0440000000011'
        native['cases'][0]['fixed'][0]['discount_ratio'] = '1.00000000000011'
        result = comparison.compare(native, reference)
        self.assertEqual({r['bound'] for r in result['errors']}, {'0.000000000001', '0.0000000000001'})

    def test_dates_and_quotation_intervals_require_exact_matches(self):
        for container, field in [('fixed', 'payment'), ('rates', 'rate_end')]:
            native, reference = synthetic_control_outputs()
            native['cases'][0][container][0][field] = '2024-07-06'
            with self.assertRaises(comparison.Failure):
                comparison.compare(native, reference)

    def test_missing_duplicate_and_reordered_cases_fail(self):
        for change in ['missing', 'duplicate', 'reordered']:
            native, reference = synthetic_control_outputs()
            if change == 'missing':
                native['cases'].pop()
            elif change == 'duplicate':
                native['cases'].append(native['cases'][0])
            else:
                native['cases'].reverse()
            with self.assertRaisesRegex(comparison.Failure, 'comparison case'):
                comparison.compare(native, reference)

    def test_missing_extra_fields_coupon_observations_and_false_origins_fail(self):
        for change in ['missing_par', 'extra_coupon', 'missing_rates', 'false_origin', 'duplicate_rate']:
            native, reference = synthetic_control_outputs()
            row = native['cases'][0]
            if change == 'missing_par':
                del row['par']['fixed_rate']
            elif change == 'extra_coupon':
                row['fixed'][0]['ignored'] = 'x'
            elif change == 'missing_rates':
                row['rates'] = []
            elif change == 'false_origin':
                row['rates'][0]['origin'] = 'Projected'
            else:
                row['rates'].append(copy.deepcopy(row['rates'][0]))
                reference['cases'][0]['rates'].append(copy.deepcopy(reference['cases'][0]['rates'][0]))
            with self.assertRaises(comparison.Failure):
                comparison.compare(native, reference)

    def test_modified_input_binding_and_engine_identity_fail(self):
        for change in ['input', 'engine']:
            native, reference = synthetic_control_outputs()
            if change == 'input':
                native['inputs']['cases'][0]['terms']['currency'] = 'GBP'
            else:
                reference['engine'] = 'independent_manual_oracle'
            with self.assertRaises(comparison.Failure):
                comparison.compare(native, reference)

    def test_fake_publisher_wheel_is_rejected_before_runtime_import(self):
        with tempfile.TemporaryDirectory() as temporary:
            fixture = comparison.inputs()
            wheel = Path(temporary) / fixture['comparator']['wheel']
            wheel.write_bytes(b'not the publisher wheel')
            with self.assertRaisesRegex(comparison.Failure, 'checksum'):
                comparison.verified_runtime(wheel, fixture)

    def test_real_failed_capture_keeps_raw_outcome_and_cannot_replay_or_overwrite(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / 'failed-run'
            result = comparison.run(Path(sys.executable), Path(temporary) / 'missing.whl', directory)
            self.assertEqual(result['status'], 'execution_incomplete')
            self.assertFalse(result['accepted_task'])
            commands = comparison.loads((directory / 'commands.json').read_bytes())
            self.assertEqual([r['name'] for r in commands], ['quantlib'])
            self.assertEqual(commands[0]['exit_code'], 2)
            self.assertIn('missing exact pinned QuantLib wheel', (directory / 'quantlib.stderr').read_text())
            with self.assertRaisesRegex(comparison.Failure, 'successful actual engine'):
                comparison.replay(directory)
            before = (directory / 'manifest.json').read_bytes()
            with self.assertRaises(FileExistsError):
                comparison.run(Path(sys.executable), Path(temporary) / 'missing.whl', directory)
            self.assertEqual((directory / 'manifest.json').read_bytes(), before)
            (directory / 'quantlib.stdout').write_text('{}')
            with self.assertRaisesRegex(comparison.Failure, 'artifact bytes'):
                comparison.replay(directory)

    def test_not_started_interpreter_is_recorded_as_a_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary) / 'no-python'
            result = comparison.run(Path(temporary) / 'no-python-executable', Path(temporary) / 'missing.whl', directory)
            self.assertEqual(result['status'], 'execution_incomplete')
            commands = comparison.loads((directory / 'commands.json').read_bytes())
            self.assertIsNone(commands[0]['exit_code'])
            self.assertEqual(commands[0]['error'], 'execution_not_started')
            self.assertTrue((directory / 'quantlib.stderr').read_bytes())


if __name__ == '__main__':
    unittest.main()
