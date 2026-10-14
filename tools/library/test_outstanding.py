"""The remaining-work census cannot omit required dimensions or certify examples."""
import unittest
from unittest.mock import patch

import outstanding


class OutstandingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.inventory = outstanding.api.read(outstanding.ROOT / outstanding.api.INVENTORY)
        cls.matrix = outstanding.api.read(outstanding.ROOT / outstanding.api.MATRIX)
        cls.report = outstanding.report()

    def test_all_original_dimensions_and_owners_survive(self):
        original = {row['id']: row for row in self.inventory['records']}
        rows = self.report['records']
        self.assertEqual(set(original), {row['record_id'] for row in rows})
        self.assertEqual(len(rows), len(original))
        for row in rows:
            required = original[row['record_id']]
            self.assertEqual(row['operation'], required['operation'])
            self.assertEqual(row['surface'], required['surface'])
            self.assertEqual(row['pending_implementation_tasks'], required['pending_task_ids'])
            self.assertTrue(row['mandatory'])
            self.assertFalse(self.report['certified'])

    def test_family_examples_cannot_supply_an_exact_mapping(self):
        routes = {**self.matrix['operation_routes'],
                  **outstanding.api.native_routes(outstanding.ROOT, self.inventory)}
        for row in self.report['records']:
            expected = sorted(k for k, route in routes.items()
                              if row['record_id'] in route['required_record_ids'])
            self.assertEqual(row['exact_operation_routes'], expected)
            if not expected and row['engineering_dispatch_status'] == 'candidate':
                self.assertEqual(row['remaining_work'], 'exact_operation_evidence_required')

    def test_exact_mapping_is_not_a_certificate(self):
        mapped = [row for row in self.report['records'] if row['exact_operation_routes']]
        self.assertTrue(mapped)
        for row in mapped:
            self.assertEqual(row['certificate_status'], 'Planned')
        self.assertFalse(self.report['accepted_task'])

    def test_missing_required_row_is_rejected(self):
        from copy import deepcopy
        matrix = deepcopy(self.matrix)
        matrix['records'].pop()
        read = outstanding.api.read
        def altered(path):
            return matrix if path.name == 'api-capabilities.json' else read(path)
        with patch.object(outstanding.api, 'read', side_effect=altered):
            with self.assertRaisesRegex(ValueError, 'mandatory record missing'):
                outstanding.report()


if __name__ == '__main__':
    unittest.main()
