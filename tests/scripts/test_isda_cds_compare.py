"""Rejection controls; synthetic control outputs never count as native evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('cds_compare', Path(__file__).with_name('isda_cds_compare.py'))
comparison = importlib.util.module_from_spec(spec)
spec.loader.exec_module(comparison)


class Controls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.grid, cls.inputs, cls.original = comparison.reference()
        reference = comparison.loads((comparison.ROOT / comparison.PACK / 'isda-reference.json').read_bytes())
        cls.synthetic = dict(schema_version=1, engine='Convex', model_version='1.8.3',
            inputs_sha256=comparison.PINS['inputs.json'], notice=comparison.NOTICE,
            cases=[dict(source, **{k: c[k] for k in (*comparison.BOUNDS, 'dirty_upfront_fraction')},
                        hazard_per_act365f_year=0.01, native_quote_residual=0.0, iterations=10)
                   for source, c in zip(cls.inputs['cases'], reference['cases'])])

    def test_actual_full_original_grid_is_not_a_certificate(self):
        self.assertEqual(self.original['status'], 'passed')
        self.assertEqual(self.original['compared_values'], 35820)
        self.assertFalse(self.original['certified'])
        self.assertEqual(len(self.inputs['discount_recipe']['source']['Nodes']), 36)

    def test_changed_original_output_license_and_input_are_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shutil.copytree(comparison.ROOT / comparison.PACK, root / comparison.PACK)
            for path in (comparison.DRIVER, comparison.LICENSE):
                target = root / path
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(comparison.ROOT / path, target)
            for name in ('isda-reference.json', 'inputs.json', 'grid.json', 'NYM.csv'):
                path = root / comparison.PACK / name
                original = path.read_bytes()
                path.write_bytes(original + b' ')
                with self.assertRaisesRegex(comparison.Failure, 'pin/bytes differ'):
                    comparison.reference(root)
                path.write_bytes(original)
            (root / comparison.LICENSE).write_bytes(b'rewritten license')
            with self.assertRaisesRegex(comparison.Failure, 'license bytes differ'):
                comparison.reference(root)

    def test_duplicate_nonfinite_oversize_and_boolean_numbers_fail(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}', b' ' * (8*1024*1024+1)):
            with self.assertRaises(comparison.Failure):
                comparison.loads(raw)
        for value in (True, None, 'NaN', 'Infinity', 'bad', '1e13'):
            with self.assertRaises(comparison.Failure):
                comparison.number(value)

    def test_controls_match_with_absolute_original_units(self):
        result = comparison.validate_candidate(self.synthetic, self.grid, self.inputs)
        self.assertEqual(result['status'], 'passed')
        self.assertFalse(result['certified'])
        candidate = copy.deepcopy(self.synthetic)
        candidate['cases'][0]['clean_price_per100'] = str(comparison.number(self.grid['cases'][0]['clean_price_per100']) + comparison.Decimal('1e-6'))
        self.assertEqual(comparison.validate_candidate(candidate, self.grid, self.inputs)['status'], 'passed')
        candidate['cases'][0]['clean_price_per100'] = str(comparison.number(candidate['cases'][0]['clean_price_per100']) + comparison.Decimal('1e-12'))
        self.assertEqual(comparison.validate_candidate(candidate, self.grid, self.inputs)['status'], 'numerical_mismatch')

    def test_case_date_economics_unit_and_conversion_controls(self):
        for field, value in [('cash_settlement_date', '2022-06-25'), ('maturity_date', '2023-06-21'),
                ('currency', 'EUR'), ('recovery', '0.4'), ('spread_bps', '100'),
                ('native_quote_residual', 1.1e-12), ('hazard_per_act365f_year', -0.1),
                ('iterations', 101), ('dirty_upfront_fraction', 0.5), ('accrued_days', float('nan'))]:
            candidate = copy.deepcopy(self.synthetic)
            candidate['cases'][0][field] = value
            with self.assertRaises(comparison.Failure):
                comparison.validate_candidate(candidate, self.grid, self.inputs)

    def test_missing_duplicate_reordered_and_extra_rows_or_measures_fail(self):
        for change in ('missing', 'duplicate', 'reordered', 'missing_measure', 'extra'):
            candidate = copy.deepcopy(self.synthetic)
            if change == 'missing': candidate['cases'].pop()
            elif change == 'duplicate': candidate['cases'][1] = candidate['cases'][0]
            elif change == 'reordered': candidate['cases'].reverse()
            elif change == 'missing_measure': del candidate['cases'][0]['accrued_premium']
            else: candidate['cases'][0]['unexpected'] = 1
            with self.assertRaises(comparison.Failure):
                comparison.validate_candidate(candidate, self.grid, self.inputs)

    def test_model_input_and_notice_are_part_of_protocol(self):
        for field, value in [('model_version', 'latest'), ('engine', 'synthetic'), ('inputs_sha256', '0'*64), ('notice', '')]:
            candidate = copy.deepcopy(self.synthetic)
            candidate[field] = value
            with self.assertRaises(comparison.Failure):
                comparison.validate_candidate(candidate, self.grid, self.inputs)

    def test_widened_bound_and_dropped_original_case_cannot_pass(self):
        grid = copy.deepcopy(self.grid)
        grid['comparison_bounds']['cash_settlement_amount']['absolute'] = '1'
        with self.assertRaisesRegex(comparison.Failure, 'bounds changed'):
            comparison.validate_candidate(self.synthetic, grid, self.inputs)
        grid = copy.deepcopy(self.grid)
        grid['cases'].pop()
        with self.assertRaisesRegex(comparison.Failure, 'published case scope'):
            comparison.validate_candidate(self.synthetic, grid, self.inputs)

    def test_rejections_remain_active_under_optimized_python(self):
        code = "import importlib.util;from pathlib import Path;s=importlib.util.spec_from_file_location('c',Path('tests/scripts/isda_cds_compare.py'));m=importlib.util.module_from_spec(s);s.loader.exec_module(m);m.require(False,'optimized rejection')"
        run = subprocess.run([sys.executable, '-O', '-c', code], cwd=comparison.ROOT, capture_output=True)
        self.assertNotEqual(run.returncode, 0)
        self.assertIn(b'optimized rejection', run.stderr)


if __name__ == '__main__':
    unittest.main()
