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
TODAY = dt.date(2026, 10, 5)

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

    def test_lifecycle_checks_sources_and_purity_cannot_be_bypassed(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [c for c in check.REQUIRED_RUNS['assurance'] if 'lifecycle' in c]
        self.assertEqual(len(commands), 5)
        commands += ['cargo test --locked -p convex-bonds -p convex-rates -p convex-credit -p convex-analytics',
                     'cargo clippy --locked -p convex-core --lib --all-features -- -D warnings',
                     'cargo clippy --locked -p convex-bonds -p convex-rates -p convex-credit -p convex-analytics -p convex --all-targets --all-features -- -D warnings']
        for command in commands:
            self.rejects(check.check_workflow, source.replace(command, 'true'))
        modules = {p.relative_to(ROOT).as_posix() for p in (ROOT/'crates/convex-analytics/src/lifecycle').glob('*.rs')}
        self.assertEqual(len(modules), 7)
        self.assertTrue(modules <= check.inventory_paths(ROOT))
        for path in modules:
            check.check_lifecycle_source((ROOT/path).read_text())
        for mutation in ['use std::fs::File;', 'let t = std::time::SystemTime::now();',
                         'use convex_engine::Engine;', 'use std::net::TcpStream;', 'let t=Utc::now();']:
            self.rejects(check.check_lifecycle_source, mutation)
        check.check_lifecycle_source('// No SystemTime or File enters the pure API.\nuse convex_core::Date;')

    def test_standalone_workflow_checks_and_sources_cannot_be_dropped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [c for c in check.REQUIRED_RUNS['assurance']
                    if 'standalone' in c or c == 'cargo test --locked -p convex -p convex-analytics -p convex-rates -p convex-credit']
        self.assertEqual(len(commands), 7)
        for command in commands:
            with self.subTest(command=command):
                self.rejects(check.check_workflow, source.replace(command, 'true'))
        paths = check.inventory_paths(ROOT)
        modules = {p.relative_to(ROOT).as_posix() for p in
                   (ROOT/'crates/convex-analytics/src/workflow').glob('*.rs')}
        self.assertEqual(len(modules), 7)
        self.assertIn('crates/convex-analytics/src/workflow/workout.rs', modules)
        self.assertTrue(modules <= paths)

    def test_library_dispatch_checks_and_purity_cannot_be_bypassed(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [c for c in check.REQUIRED_RUNS['assurance']
                    if 'library_workflow' in c or 'library_operations' in c or 'library_cash' in c or 'cash_quotation_reference' in c or 'library_schema' in c or 'library/api.py' in c
                    or '--test library' in c or 'convex-wasm -p convex-mcp' in c
                    or 'convex-library-schema' in c or 'requirements-schema' in c or 'library_ufr' in c]
        self.assertEqual(len(commands), 31)
        for command in commands:
            with self.subTest(command=command):
                self.rejects(check.check_workflow, source.replace(command, 'true'))
        modules = {p.relative_to(ROOT).as_posix() for p in (ROOT/'crates/convex/src/library').glob('*.rs')}
        self.assertEqual(len(modules), 15)
        self.assertIn('crates/convex/src/library/workout.rs', modules)
        self.assertTrue(modules <= check.inventory_paths(ROOT))
        for path in modules:
            check.check_lifecycle_source((ROOT/path).read_text())

    def test_curve_family_checks_and_native_owners_cannot_be_bypassed(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [c for c in check.REQUIRED_RUNS['assurance']
                    if 'curve_family' in c or 'curve_families' in c]
        self.assertEqual(len(commands), 7)
        for command in commands:
            with self.subTest(command=command):
                self.rejects(check.check_workflow, source.replace(command, 'true'))
        for path in ['crates/convex-curves/src/contracts/structured.rs',
                     'crates/convex-curves/src/contracts/parametric_fit.rs',
                     'crates/convex-curves/src/curves/segmented.rs',
                     'crates/convex-curves/src/curves/delegated.rs']:
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_curve_method_checks_and_native_owners_cannot_be_bypassed(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [c for c in check.REQUIRED_RUNS['assurance'] if 'curve_method' in c]
        self.assertEqual(len(commands), 7)
        for command in commands:
            with self.subTest(command=command):
                self.rejects(check.check_workflow, source.replace(command, 'true'))
        for path in ['crates/convex-curves/src/contracts/grid.rs',
                     'crates/convex-math/src/interpolation/piecewise_constant.rs']:
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_ufr_checks_and_original_native_boundary_owner_cannot_be_bypassed(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [c for c in check.REQUIRED_RUNS['assurance'] if 'ufr' in c]
        self.assertEqual(len(commands), 9)
        for command in commands:
            with self.subTest(command=command):
                self.rejects(check.check_workflow, source.replace(command, 'true'))
        for path in ['crates/convex-curves/src/contracts/ufr.rs',
                     'crates/convex-math/src/extrapolation/ufr_convergence.rs']:
            self.assertIn(path, check.inventory_paths(ROOT))

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
        for field, value in [('owner',''), ('expires_on','2026-10-05'), ('expires_on','2027-10-04'), ('reviewed_on','2026-10-06')]:
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

    def test_declared_checkout_line_endings_do_not_hide_economic_edits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = 'crates/convex-ffi/tests/control.cs'
            file = root/path
            file.parent.mkdir(parents=True)
            file.write_bytes(b'price = 100;\r\n')
            numeric = {'version': 1, 'artifacts': [{'path': path, 'mode': 'file', 'sha256': check.digest(file.read_bytes())}]}
            review = {'schema_version': 1, 'reviews': [{'base_commit': 'base', 'actor': 'coding agent',
                'rationale': 'Physical bytes remain original', 'reviewed_on': str(TODAY), 'tests': ['regression'],
                'candidate_files': {path: check.digest(file.read_bytes())}}]}
            def baseline(_, __, name):
                return json.dumps(numeric).encode() if name.endswith('numeric-evidence.json') else b'price = 100;\n'
            with patch.object(check, 'git', return_value=path.encode()), patch.object(check, 'git_source', side_effect=baseline), patch.object(check, 'git_checkout', return_value=b'price = 100;\r\n'):
                self.assertEqual(check.check_changes(root, 'base', review, numeric, TODAY), 1)
                file.write_bytes(b'price = 101;\r\n')
                review['reviews'][0]['candidate_files'][path] = check.digest(file.read_bytes())
                self.rejects(check.check_changes, root, 'base', review, numeric, TODAY)
            file.write_bytes(b'price = 100;\r\n')
            review['reviews'][0]['candidate_files'][path] = check.digest(file.read_bytes())
            with patch.object(check, 'git', return_value=path.encode()), patch.object(check, 'git_source', side_effect=baseline), patch.object(check, 'git_checkout', return_value=None):
                self.rejects(check.check_changes, root, 'base', review, numeric, TODAY)

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

    def test_library_inventory_and_consumer_cannot_be_skipped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for mutation in [source.replace('-s tools/library', '-s tools/unreviewed'),
                         source.replace('--example inventory_probe --no-default-features', '--example inventory_probe --no-default-features || true'),
                         source.replace('--example inventory_probe --features schemars', '--example unrelated_probe --features schemars')]:
            self.rejects(check.check_workflow, mutation)
        self.assertTrue(check.protected('tools/library/library-inventory.json'))
        self.assertTrue(check.protected('tools/library/check.py'))

    def test_supplied_swap_offline_checks_cannot_be_skipped_or_ignore_failure(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [
            'python3 -m unittest discover -s tests/scripts -p test_swap_quantlib_compare.py',
            'cargo build --locked -p convex --example swap_comparison',
            'python3 tests/scripts/swap_quantlib_compare.py offline --candidate-binary target/debug/examples/swap_comparison --report swap-comparison-report.json',
        ]
        for command in commands:
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: '+command+' || true\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'if: false\n        run: '+command+'\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: echo skipped\n'))

    def test_supplied_swap_raw_outputs_must_be_uploaded_even_on_failure(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        self.rejects(check.check_workflow, source.replace('            swap-comparison-report-native.stdout\n', ''))
        self.rejects(check.check_workflow, source.replace('            swap-comparison-report-native.stderr\n', ''))
        self.rejects(check.check_workflow, source.replace('Upload supplied swap comparison report\n        if: always()',
                                                        'Upload supplied swap comparison report\n        if: success()'))

    def test_cds_native_grid_and_consumer_checks_cannot_be_skipped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'cargo test --locked -p convex-credit',
            'cargo clippy --locked -p convex-credit --all-targets -- -D warnings',
            'python3 tests/scripts/isda_cds_compare.py check',
            'python3 -m unittest discover -s tests/scripts -p test_isda_cds_compare.py',
            'cargo build --locked -p convex-credit --example cds_comparison',
            'python3 tests/scripts/isda_cds_compare.py offline --candidate-binary target/debug/examples/cds_comparison --report cds-comparison-report.json',
            'cargo run --locked -p convex --example cds_valuation',
            'cargo run --locked -p convex --example cds_valuation --no-default-features',
            'cargo run --locked -p convex --example cds_valuation --features schemars',
        ]:
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: '+command+' || true\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'if: false\n        run: '+command+'\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: echo skipped\n'))
        for path in ('crates/convex-credit/src/pricing.rs', 'tests/scripts/isda_cds_compare.py',
                     'tests/scripts/isda_cds_capture.c', 'licenses/ISDA-CDS-Standard-Model-Public-License-1.0.txt'):
            self.assertTrue(check.protected(path))
        self.assertIn('convex-credit',check.DOMAIN)

    def test_cds_raw_outputs_must_be_uploaded_on_failure(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        self.rejects(check.check_workflow, source.replace('            cds-comparison-report-native.stdout\n', ''))
        self.rejects(check.check_workflow, source.replace('            cds-comparison-report-native.stderr\n', ''))
        self.rejects(check.check_workflow, source.replace('Upload supplied CDS comparison report\n        if: always()',
                                                        'Upload supplied CDS comparison report\n        if: success()'))

    def test_shared_cashflow_consumer_feature_checks_are_mandatory(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'cargo run --locked -p convex --example cashflow_primitives',
            'cargo run --locked -p convex --example cashflow_primitives --no-default-features',
            'cargo run --locked -p convex --example cashflow_primitives --features schemars',
        ]:
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: '+command+' || true\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'if: false\n        run: '+command+'\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: cargo run --example unrelated_consumer\n'))

    def test_credit_calibration_risk_checks_and_raw_outputs_are_mandatory(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'cargo test --locked -p convex-analytics --test credit_calibration',
            'python3 tests/scripts/isda_credit_compare.py check',
            'cargo run --locked -p convex --example credit_calibration',
            'cargo run --locked -p convex --example credit_calibration --no-default-features',
            'cargo run --locked -p convex --example credit_calibration --features schemars',
            'python3 -m unittest discover -s tests/scripts -p test_isda_credit_compare.py',
            'cargo build --locked -p convex-analytics --example credit_comparison',
            'python3 tests/scripts/isda_credit_compare.py offline --candidate-binary target/debug/examples/credit_comparison --report credit-comparison-report.json',
        ]:
            for replacement in ['run: '+command+' || true\n', 'if: false\n        run: '+command+'\n', 'run: echo skipped\n']:
                self.rejects(check.check_workflow, source.replace('run: '+command+'\n', replacement))
        for suffix in ['stdout', 'stderr']:
            self.rejects(check.check_workflow, source.replace('            credit-comparison-report-native.'+suffix+'\n', ''))
        self.rejects(check.check_workflow, source.replace('Upload supplied credit comparison report\n        if: always()',
                                                        'Upload supplied credit comparison report\n        if: success()'))
        for path in ['crates/convex-analytics/src/credit_calibration.rs',
                     'crates/convex-analytics/src/credit_risk.rs',
                     'tests/scripts/isda_credit_compare.py', 'tests/scripts/isda_credit_capture.c',
                     'tests/scripts/isda_credit_capture.py', 'tests/scripts/test_isda_credit_compare.py']:
            self.assertTrue(check.protected(path))

    def test_futures_reference_checks_and_consumer_features_cannot_be_skipped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [
            'cargo test --locked -p convex-analytics -p convex-ports -p convex-ext-file -p convex-ext-redb --test futures_reference',
            'cargo clippy --locked -p convex-analytics -p convex-ports -p convex-ext-file -p convex-ext-redb --test futures_reference --all-features -- -D warnings',
            'cargo run --locked -p convex --example futures_reference',
            'cargo run --locked -p convex --example futures_reference --no-default-features',
            'cargo run --locked -p convex --example futures_reference --features schemars',
        ]
        for command in commands:
            for replacement in ['run: '+command+' || true\n', 'if: false\n        run: '+command+'\n', 'run: echo skipped\n']:
                self.rejects(check.check_workflow, source.replace('run: '+command+'\n', replacement))
        for path in ['crates/convex-core/src/futures/eligibility.rs',
                     'crates/convex-ports/src/futures_reference.rs',
                     'crates/convex-ext-file/src/reference_repository.rs',
                     'crates/convex-ext-redb/src/reference_repository.rs',
                     'crates/convex-analytics/src/risk/hedging/types.rs']:
            self.assertTrue(check.protected(path))
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_futures_valuation_checks_cannot_be_skipped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'cargo test --locked -p convex-analytics --test futures_valuation',
            'cargo clippy --locked -p convex-analytics --test futures_valuation --all-features -- -D warnings',
            'cargo run --locked -p convex --example futures_valuation',
            'cargo run --locked -p convex --example futures_valuation --no-default-features',
            'cargo run --locked -p convex --example futures_valuation --features schemars',
        ]:
            for replacement in ['run: '+command+' || true\n', 'if: false\n        run: '+command+'\n', 'run: echo skipped\n']:
                self.rejects(check.check_workflow, source.replace('run: '+command+'\n', replacement))
        for name in ('mod', 'bond', 'rates', 'cash', 'settlement'):
            path = 'crates/convex-analytics/src/futures/'+name+'.rs'
            self.assertTrue(check.protected(path))
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_mixed_rate_references_rebuild_checks_and_consumer_features_cannot_be_skipped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'python3 tests/scripts/rate_calibration_reference.py check',
            'cargo test --locked -p convex-analytics --test rate_calibration',
            'cargo clippy --locked -p convex-analytics --test rate_calibration --all-features -- -D warnings',
            'cargo test --locked -p convex-curves -p convex-rates -p convex-math',
            'cargo run --locked -p convex --example rate_calibration',
            'cargo run --locked -p convex --example rate_calibration --no-default-features',
            'cargo run --locked -p convex --example rate_calibration --features schemars',
        ]:
            for replacement in ['run: '+command+' || true\n', 'if: false\n        run: '+command+'\n', 'run: echo skipped\n']:
                self.rejects(check.check_workflow, source.replace('run: '+command+'\n', replacement))
        for path in ['crates/convex-rates/src/basis.rs', *[
            'crates/convex-analytics/src/rate_calibration/'+name+'.rs'
            for name in ('mod', 'build', 'fit', 'helpers', 'inputs')]]:
            self.assertTrue(check.protected(path))
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_full_cash_reference_rebuilds_and_consumers_cannot_be_skipped(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = ['python3 tests/scripts/cash_calibration_reference.py check', 'cargo test --locked -p convex-analytics --test cash_calibration', 'cargo clippy --locked -p convex-curves -p convex-credit -p convex-analytics --all-targets --all-features -- -D warnings', 'cargo test --locked -p convex-curves -p convex-analytics -p convex-credit', 'cargo run --locked -p convex --example cash_calibration', 'cargo run --locked -p convex --example cash_calibration --no-default-features', 'cargo run --locked -p convex --example cash_calibration --features schemars']
        for command in commands:
            for replacement in ['run: '+command+' || true\n', 'if: false\n        run: '+command+'\n', 'run: echo skipped\n']:
                self.rejects(check.check_workflow, source.replace('run: '+command+'\n', replacement))
        for path in ['crates/convex-curves/src/contracts/basis.rs', *[
                'crates/convex-analytics/src/cash_calibration/'+name+'.rs'
                for name in ('mod', 'build', 'fit', 'inputs', 'pricing', 'repo')]]:
            self.assertTrue(check.protected(path))
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_signed_risk_reference_full_graph_coverage_and_consumers_are_mandatory(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        commands = [
            'python3 tests/scripts/quote_risk_reference.py check',
            'cargo test --locked -p convex-analytics --test quote_risk',
            'cargo test --locked -p convex-portfolio --test signed_risk',
            'cargo clippy --locked -p convex-curves -p convex-analytics -p convex-portfolio -p convex --all-targets --all-features -- -D warnings',
            'cargo test --locked -p convex-analytics -p convex-portfolio -p convex-curves',
            'cargo run --locked -p convex --example quote_risk',
            'cargo run --locked -p convex --example quote_risk --no-default-features',
            'cargo run --locked -p convex --example quote_risk --features schemars',
            'cargo run --locked -p convex-portfolio --example signed_risk',
        ]
        for command in commands:
            for replacement in ['run: '+command+' || true\n', 'if: false\n        run: '+command+'\n', 'run: echo skipped\n']:
                self.rejects(check.check_workflow, source.replace('run: '+command+'\n', replacement))
        for path in ['crates/convex-portfolio/src/signed_risk.rs', 'crates/convex-core/src/contracts/risk.rs', *[
                'crates/convex-analytics/src/quote_risk/'+name+'.rs'
                for name in ('mod', 'types', 'finite', 'graph', 'optional')]]:
            self.assertTrue(check.protected(path))
            self.assertIn(path, check.inventory_paths(ROOT))

    def test_curve_consumer_feature_checks_are_mandatory(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'cargo run --locked -p convex --example curve_contracts',
            'cargo run --locked -p convex --example curve_contracts --no-default-features',
            'cargo run --locked -p convex --example curve_contracts --features schemars',
        ]:
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: '+command+' || true\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'if: false\n        run: '+command+'\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: cargo run --example unrelated_consumer\n'))

    def test_curve_numerics_feature_checks_are_mandatory(self):
        source = (ROOT/'.github/workflows/ci.yml').read_text()
        for command in [
            'cargo run --locked -p convex --example curve_numerics',
            'cargo run --locked -p convex --example curve_numerics --no-default-features',
            'cargo run --locked -p convex --example curve_numerics --features schemars',
        ]:
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: '+command+' || true\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'if: false\n        run: '+command+'\n'))
            self.rejects(check.check_workflow, source.replace('run: '+command+'\n', 'run: cargo run --example unrelated_consumer\n'))

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
