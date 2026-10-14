"""Native-only evidence cannot conceal required JSON/C work or certify scope."""
import copy
import unittest
import api


class NativeOperationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.inventory = api.read(api.ROOT / api.INVENTORY)
        cls.document = api.read(api.ROOT / api.NATIVE_ROUTES)

    def rejects(self, mutate):
        candidate = copy.deepcopy(self.document)
        mutate(candidate)
        with self.assertRaises((ValueError, KeyError)):
            api.native_routes(inventory=self.inventory, document=candidate)

    def row(self, candidate):
        return next(iter(candidate['routes'].values()))

    def test_all_eight_original_rust_only_operations_have_concrete_controls(self):
        routes = api.native_routes(inventory=self.inventory, document=self.document)
        expected = {r['id'] for r in self.inventory['records'] if r['id'].startswith(('LC-001/', 'LC-002/'))}
        self.assertEqual({record for route in routes.values() for record in route['required_record_ids']}, expected)
        self.assertEqual(len(expected), 8)
        self.assertFalse(self.document['certified'])

    def test_native_proof_cannot_replace_a_required_shared_surface(self):
        record = next(r['id'] for r in self.inventory['records'] if r['surface'] == 'shared_json')
        self.rejects(lambda d: self.row(d).update(required_record_ids=[record]))
        record = next(r['id'] for r in self.inventory['records'] if r['surface'] == 'rust'
                      and r['id'].startswith('LC-003/'))
        self.rejects(lambda d: self.row(d).update(required_record_ids=[record]))

    def test_missing_profile_or_wrong_command_cannot_qualify(self):
        self.rejects(lambda d: d['build_commands'].pop('rust-no-default'))
        self.rejects(lambda d: d['build_commands']['rust-schemars'][0].remove('schemars'))

    def test_enum_only_or_nonexistent_method_and_control_are_rejected(self):
        self.rejects(lambda d: self.row(d).update(entry_points=[]))
        self.rejects(lambda d: self.row(d).update(entry_points=['convex::cashflows::invented']))
        self.rejects(lambda d: self.row(d)['positive_controls'][0].update(function='invented_control'))
        self.rejects(lambda d: self.row(d).update(anchors=[]))

    def test_all_outcome_kinds_and_original_records_are_required(self):
        for field in ['positive_controls', 'boundary_controls', 'negative_controls']:
            self.rejects(lambda d: self.row(d).update({field: []}))
        self.rejects(lambda d: self.row(d).update(required_record_ids=['invented/rust']))

    def test_metadata_cannot_promote_approval_or_hide_source_paths(self):
        self.rejects(lambda d: d.update(certified=True))
        self.rejects(lambda d: self.row(d).update(certified=True))
        self.rejects(lambda d: self.row(d)['anchors'][0].update(path='../../outside.rs'))
        self.rejects(lambda d: self.row(d)['positive_controls'][0].update(path='../../outside.rs'))


if __name__ == '__main__':
    unittest.main()
