"""Mutation checks for assurance rules; no network, cargo, or external services."""
import copy
import datetime as dt
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import check

ROOT = Path(__file__).resolve().parents[2]
TODAY = dt.date(2026, 10, 4)

class AssuranceMutations(unittest.TestCase):
    def rejects(self, function, *args):
        with self.assertRaises(check.Failure):
            function(*args)

    def metadata(self, kind=None, target=None, dependency='tokio'):
        return {'workspace_members':['domain'], 'packages':[{'id':'domain','name':'domain'}, {'id':'dep','name':dependency}],
            'resolve':{'nodes':[{'id':'domain','deps':[{'pkg':'dep','dep_kinds':[{'kind':kind,'target':target}]}]}, {'id':'dep','deps':[]}]}}

    def test_normal_and_target_specific_network_edges_are_included(self):
        for target in [None, 'cfg(windows)']:
            with self.subTest(target=target):
                names = check.normal_closure(self.metadata(target=target), ('domain',))
                self.assertIn('tokio', names)

    def test_dev_and_build_tools_are_not_domain_dependencies(self):
        for kind in ['dev','build']:
            self.assertEqual(check.normal_closure(self.metadata(kind=kind), ('domain',)), {'domain'})

    def test_all_feature_domain_dependencies_need_review(self):
        packages = [{'id':name,'name':name} for name in check.DOMAIN]
        nodes = [{'id':name,'deps':[]} for name in check.DOMAIN]
        packages.append({'id':'dep','name':'new-storage-impl'})
        nodes[0]['deps'] = [{'pkg':'dep','dep_kinds':[{'kind':None,'target':None}]}]
        nodes.append({'id':'dep','deps':[]})
        metadata = {'workspace_members':list(check.DOMAIN), 'packages':packages, 'resolve':{'nodes':nodes}}
        self.rejects(check.check_dependencies, metadata, set(check.DOMAIN))
        metadata['packages'][-1]['name'] = 'tokio'
        self.rejects(check.check_dependencies, metadata, {*check.DOMAIN,'tokio'})

    def test_workflow_rejects_removed_jobs_or_checks(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        check.check_workflow(source)
        self.rejects(check.check_workflow, source.replace('  assurance:', '  obsolete:'))
        self.rejects(check.check_workflow, source.replace('cargo fmt --all -- --check', 'cargo fmt --all'))

    def test_workflow_rejects_conditionals_ignored_exits_and_optout(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for mutation in [source.replace('    name: Bounded Rust Assurance', '    if: false\n    name: Bounded Rust Assurance'),
                         source.replace('        run: cargo fmt --all -- --check', '        continue-on-error: true\n        run: cargo fmt --all -- --check'),
                         source.replace('cargo fmt --all -- --check', 'cargo fmt --all -- --check || true'),
                         source.replace("CONVEX_PRODUCTION_PROBES: '1'", "CONVEX_PRODUCTION_PROBES: '0'"),
                         source.replace('  pull_request:', '  workflow_dispatch:'),
                         source.replace('  push:', '  push:\n    paths-ignore: [crates/**]')]:
            self.rejects(check.check_workflow, mutation)

    def test_pr_exception_cannot_move_to_another_required_step(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        mutation = source.replace("        if: github.event_name == 'pull_request'\n", '')
        mutation = mutation.replace('        run: cargo fmt --all -- --check', "        if: github.event_name == 'pull_request'\n        run: cargo fmt --all -- --check")
        self.rejects(check.check_workflow, mutation)

    def test_expiry_owner_and_duplicate_exceptions(self):
        source = check.read_json(ROOT/'tools/assurance/exceptions.json')
        check.check_exceptions(source, TODAY)
        for field, value in [('owner',''), ('expires_on','2026-10-04'), ('expires_on','2027-10-04'), ('reviewed_on','2026-10-05')]:
            candidate = copy.deepcopy(source)
            candidate['exceptions'][0][field] = value
            self.rejects(check.check_exceptions, candidate, TODAY)
        candidate = copy.deepcopy(source)
        candidate['exceptions'].append(candidate['exceptions'][0])
        self.rejects(check.check_exceptions, candidate, TODAY)

    def test_duplicate_json_is_not_silently_collapsed(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'policy.json'
            path.write_text('{"version":1,"version":2}')
            self.rejects(check.read_json, path)

    def test_source_inventory_rejects_stale_hash_and_missing_path(self):
        inventory = check.read_json(ROOT/'tools/assurance/inventory.json')
        identifiers = check.check_exceptions(check.read_json(ROOT/'tools/assurance/exceptions.json'), TODAY)
        check.check_inventory(ROOT, inventory, identifiers)
        candidate = copy.deepcopy(inventory)
        candidate['files'][0]['sha256'] = '0'*64
        self.rejects(check.check_inventory, ROOT, candidate, identifiers)
        candidate = copy.deepcopy(inventory)
        candidate['files'].pop()
        self.rejects(check.check_inventory, ROOT, candidate, identifiers)
        candidate = copy.deepcopy(inventory)
        candidate['c_exports'].pop()
        self.rejects(check.check_inventory, ROOT, candidate, identifiers)

    def test_quantitative_pin_and_default_reject_silent_changes(self):
        source = check.read_json(ROOT/'tools/assurance/numeric-evidence.json')
        check.check_numeric(ROOT, source)
        candidate = copy.deepcopy(source)
        candidate['artifacts'][0]['sha256'] = '0'*64
        self.rejects(check.check_numeric, ROOT, candidate)
        candidate = copy.deepcopy(source)
        candidate['solver_defaults']['DEFAULT_TOLERANCE'] = '1e-6'
        self.rejects(check.check_numeric, ROOT, candidate)

    def test_comparison_pins_and_required_offline_job_cannot_be_removed(self):
        numeric = check.read_json(ROOT/'tools/assurance/numeric-evidence.json')
        for path in check.COMPARISON_PINS:
            candidate = copy.deepcopy(numeric)
            candidate['artifacts'] = [r for r in candidate['artifacts'] if r['path'] != path]
            self.rejects(check.check_numeric, ROOT, candidate)
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for mutation in [source.replace('  offline-comparison:', '  old-comparison:'),
                         source.replace('    name: Offline Quantitative Comparison', '    if: false\n    name: Offline Quantitative Comparison'),
                         source.replace('--report comparison-report.json', '--report comparison-report.json || true'),
                         source.replace('      - name: Replay retained QuantLib expectations without live data',
                                        '      - name: Replay retained QuantLib expectations without live data\n        if: always()')]:
            self.rejects(check.check_workflow, mutation)

    def change_fixture(self, directory):
        root = Path(directory)
        path = 'tests/fixtures/example.json'
        (root/'tests/fixtures').mkdir(parents=True)
        (root/path).write_text('{"tolerance":0.1}')
        numeric = {'schema_version':1, 'version':1, 'artifacts':[{'path':path,'mode':'file','sha256':check.digest((root/path).read_bytes())}]}
        review = {'schema_version':1,'reviews':[{'base_commit':'base','actor':'coding-agent','rationale':'explicit impact',
            'reviewed_on':TODAY.isoformat(),'tests':['numeric comparison'], 'candidate_files':{path:check.digest((root/path).read_bytes())}}]}
        return root, path, numeric, review

    def test_untracked_and_stale_impact_records_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root, path, numeric, review = self.change_fixture(directory)
            with patch.object(check,'git',return_value=path.encode()), patch.object(check,'git_source',return_value=None):
                self.assertEqual(check.check_changes(root,'base',review,numeric,TODAY),1)
                self.rejects(check.check_changes, root,'base',{'schema_version':1,'reviews':[]},numeric,TODAY)
                (root/path).write_text('{"tolerance":1.0}')
                self.rejects(check.check_changes, root,'base',review,numeric,TODAY)

    def test_refreshed_fixture_hash_still_needs_independent_review(self):
        with tempfile.TemporaryDirectory() as directory:
            root, path, numeric, review = self.change_fixture(directory)
            old = b'{"tolerance":0.0001}'
            def baseline(_, revision, requested): return old if requested == path else None
            with patch.object(check,'git',return_value=path.encode()), patch.object(check,'git_source',side_effect=baseline):
                self.rejects(check.check_changes, root,'base',review,numeric,TODAY)
                row = review['reviews'][0]
                row['quantitative_review'] = {'reviewer':'coding-agent','rationale':'looks fine','reference_evidence':['independent.json']}
                self.rejects(check.check_changes, root,'base',review,numeric,TODAY)
                row['quantitative_review']['reviewer'] = 'declared-financial-owner'
                self.assertEqual(check.check_changes(root,'base',review,numeric,TODAY),1)

    def test_removed_quantitative_pin_needs_version_and_review(self):
        with tempfile.TemporaryDirectory() as directory:
            root, path, numeric, review = self.change_fixture(directory)
            previous = copy.deepcopy(numeric)
            previous['artifacts'].append({'path':'old-fixture.json','mode':'file','sha256':'0'*64})
            def baseline(_, revision, requested): return json.dumps(previous).encode() if requested == 'tools/assurance/numeric-evidence.json' else None
            with patch.object(check,'git',return_value=path.encode()), patch.object(check,'git_source',side_effect=baseline):
                self.rejects(check.check_changes, root,'base',review,numeric,TODAY)
                numeric['version'] = 2
                self.rejects(check.check_changes, root,'base',review,numeric,TODAY)

    def test_python_optimization_does_not_disable_rejections(self):
        result = subprocess.run([sys.executable, '-O', '-c', 'import check; check.require(False, "required check")'], cwd=ROOT/'tools/assurance', capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'required check', result.stderr)

if __name__ == '__main__':
    unittest.main()
