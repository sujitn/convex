"""Coverage metadata rejection tests; these doubles do not certify analytics."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("library_coverage", Path(__file__).with_name("check.py"))
coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage)


class CoverageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = coverage.load()

    def candidate(self):
        return copy.deepcopy(self.document)

    def test_every_area_operation_and_surface_has_concrete_records(self):
        coverage.validate(self.document)
        self.assertEqual(len(self.document["requirement_areas"]), 30)
        self.assertEqual(len(self.document["combinations"]), 174)
        self.assertEqual(len(self.document["records"]), 2154)
        for gate in (None, "LB0", "LB1"):
            report = coverage.report(self.document, gate)
            self.assertFalse(report["certified"])
            self.assertEqual(report["mandatory_records"], report["blocked_records"])

    def test_real_type_only_and_missing_dispatch_are_gaps_ac1(self):
        report = coverage.report(self.document)
        areas = {r["id"]: r for r in report["areas"]}
        self.assertIn("type_only", areas["LC-017"]["gaps"])
        self.assertIn("missing_dispatch", areas["LC-015"]["gaps"])
        self.assertIn("lossy_schema", areas["LC-028"]["gaps"])
        self.assertIn("approximation", areas["LC-023"]["gaps"])
        self.assertEqual(self.document["observations"]["basis"]["assessment"], "type_only")
        self.assertEqual(self.document["observations"]["ffi_piecewise"]["assessment"], "missing_dispatch")

    def test_a_declaration_or_missing_dispatch_cannot_be_promoted(self):
        for row in self.document["records"]:
            if row["combination_id"].startswith(("LC-017/", "LC-015/core_dated_helpers/SUPPLIED/piecewise")):
                for status in ("Verified", "ImplementedUnverified"):
                    with self.subTest(record=row["id"], status=status):
                        candidate = self.candidate()
                        index = candidate["records"].index(row)
                        candidate["records"][index]["status"] = status
                        with self.assertRaises(coverage.Failure):
                            coverage.validate(candidate)

    def test_every_mandatory_record_has_pending_owner_ac2(self):
        areas = {a["id"]: a for a in self.document["requirement_areas"]}
        combos = {c["id"]: c for c in self.document["combinations"]}
        for row in self.document["records"]:
            area = areas[combos[row["combination_id"]]["area_id"]]
            self.assertTrue(row["pending_task_ids"])
            self.assertLessEqual(set(row["pending_task_ids"]), set(area["delivery_task_ids"]))
        candidate = self.candidate()
        candidate["records"][0]["pending_task_ids"] = []
        with self.assertRaises(coverage.Failure):
            coverage.validate(candidate)

    def test_inapplicable_and_deferred_need_reasons_ac3(self):
        for change in ("reason", "mandatory", "status"):
            candidate = self.candidate()
            row = candidate["outside_or_inapplicable"][0]
            row[change] = {"reason": "", "mandatory": True, "status": "Verified"}[change]
            with self.subTest(change=change), self.assertRaises(coverage.Failure):
                coverage.validate(candidate)

    def test_mandatory_status_and_identity_cannot_be_waived(self):
        for status in ("NotApplicable", "Deferred", "Verified", "Unknown"):
            candidate = self.candidate()
            candidate["records"][0]["status"] = status
            candidate["records"][0]["reason"] = "Attempted waiver"
            with self.subTest(status=status), self.assertRaises(coverage.Failure):
                coverage.validate(candidate)
        candidate = self.candidate()
        candidate["records"][0]["mandatory"] = False
        with self.assertRaises(coverage.Failure):
            coverage.validate(candidate)

    def test_missing_record_area_surface_and_profile_rejected(self):
        candidates = []
        candidate = self.candidate(); candidate["records"].pop(); candidates.append(candidate)
        candidate = self.candidate(); candidate["requirement_areas"].pop(); candidates.append(candidate)
        candidate = self.candidate(); candidate["combinations"][4]["surfaces"] = []; candidates.append(candidate)
        candidate = self.candidate(); candidate["combinations"][0]["build_profiles"].pop(); candidates.append(candidate)
        for i, candidate in enumerate(candidates):
            with self.subTest(mutation=i), self.assertRaises(coverage.Failure):
                coverage.validate(candidate)

    def test_an_entire_advertised_host_surface_cannot_disappear(self):
        candidate = self.candidate()
        removed = {c['id'] for c in candidate['combinations'] if c['surfaces'] == ['mcp_advertised']}
        candidate['combinations'] = [c for c in candidate['combinations'] if c['id'] not in removed]
        candidate['records'] = [r for r in candidate['records'] if r['combination_id'] not in removed]
        with self.assertRaisesRegex(coverage.Failure, 'advertised client'):
            coverage.validate(candidate)

    def test_coverage_deletion_survives_version_refresh_rejected(self):
        candidate = self.candidate()
        combo = candidate["combinations"].pop(2)
        candidate["records"] = [r for r in candidate["records"] if r["combination_id"] != combo["id"]]
        candidate["inventory_version"] += 1
        coverage.validate(candidate)  # Another combination still covers this area.
        with self.assertRaisesRegex(coverage.Failure, "coverage removed"):
            coverage.preserve(self.document, candidate)

    def test_semantic_scope_cannot_be_reinterpreted(self):
        for section, field, value in (("combinations", "economic_terms", "average coupon"),
                                      ("requirement_areas", "minimum_gate", "LB1")):
            candidate = self.candidate()
            candidate[section][0][field] = value
            candidate["inventory_version"] += 1
            with self.subTest(section=section), self.assertRaises(coverage.Failure):
                coverage.preserve(self.document, candidate)
        candidate = self.candidate()
        candidate["models"]["credit"]["numerical_policy"] = "Use Z-spread"
        with self.assertRaises(coverage.Failure):
            coverage.preserve(self.document, candidate)

    def test_source_drift_and_missing_anchor_rejected(self):
        candidate = self.candidate()
        candidate["source_pins"][next(iter(candidate["source_pins"]))] = "0" * 64
        with self.assertRaisesRegex(coverage.Failure, "stale inspected source"):
            coverage.validate(candidate)
        candidate = self.candidate()
        candidate["observations"]["basis"]["anchors"][0]["text"] = "missing-real-implementation"
        with self.assertRaisesRegex(coverage.Failure, "missing inspected anchor"):
            coverage.validate(candidate)

    def test_duplicate_keys_ids_and_nonfinite_are_rejected(self):
        for data in (b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}'):
            with self.assertRaises(coverage.Failure):
                coverage.decode(data)
        for section in ("requirement_areas", "records", "combinations", "outside_or_inapplicable"):
            candidate = self.candidate()
            candidate[section].append(candidate[section][0])
            with self.subTest(section=section), self.assertRaises(coverage.Failure):
                coverage.validate(candidate)

    def test_unsafe_evidence_paths_and_symlink_escape_rejected(self):
        for path in ("../outside", "/etc/passwd"):
            with self.assertRaises(coverage.Failure):
                coverage.file_path(coverage.ROOT, path)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "escape").symlink_to(Path(__file__).resolve())
            with self.assertRaises(coverage.Failure):
                coverage.file_path(root, "escape")

    def test_lb0_and_lb1_cli_fail_without_awarding_certificate(self):
        for gate in ("LB0", "LB1"):
            result = subprocess.run([sys.executable, "-O", str(coverage.ROOT / "tools/library/check.py"), "--gate", gate], capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report["status"], "incomplete")
            self.assertFalse(report["certified"])

    def test_bad_manifest_replaces_stale_success_report(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tools/library").mkdir(parents=True)
            (root / coverage.MANIFEST).write_text('{"schema_version":0}')
            report = root / "report.json"
            report.write_text('{"status":"passed","certified":true}')
            result = subprocess.run([sys.executable, "-O", str(coverage.ROOT / "tools/library/check.py"), "--root", str(root), "--report", str(report)], capture_output=True)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(json.loads(report.read_text())["status"], "rejected")
            self.assertFalse(json.loads(report.read_text())["certified"])

    def test_noop_base_preserves_and_changed_metadata_needs_version(self):
        coverage.preserve(self.document, self.document)
        candidate = self.candidate()
        candidate["reviewed_at"] = "2026-10-06"
        with self.assertRaisesRegex(coverage.Failure, "increment"):
            coverage.preserve(self.document, candidate)


if __name__ == "__main__":
    unittest.main()
