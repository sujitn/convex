"""Fault/mutation tests for comparison infrastructure, not external-engine runs."""
import copy
import datetime as dt
from decimal import Decimal
import json
from pathlib import Path
import random
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import comparison as cmp

ROOT = Path(__file__).resolve().parents[1]
TODAY = dt.date(2026, 10, 4)


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.pack = cmp.read(ROOT / "tests/fixtures/comparisons/quantlib-1.40.json")
        self.pack["cases"] = self.pack["cases"][:2]
        for artifact in self.pack["artifacts"].values():
            target = self.root / artifact["path"]
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / artifact["path"], target)

    def candidate(self, pack=None):
        pack = pack or self.pack
        outputs = []
        for case in pack["cases"]:
            metrics = {}
            for name, row in case["metrics"].items():
                ref = row["reference"]
                source = cmp.read(self.root / pack["artifacts"][ref["artifact"]]["path"])
                metrics[name] = {"value": str(cmp.pointer(source, ref["pointer"])), "unit": row["unit"]}
            outputs.append({"id": case["id"], "context_hash": cmp.context(case), "status": "ok",
                            "conventions": copy.deepcopy(case["conventions"]), "metrics": metrics, "error": None})
        return {"schema_version": 1, "producer": {"name": "infrastructure test double", "version": "1", "approximations": []}, "cases": outputs}

    def rejects(self, function, *args):
        with self.assertRaises(cmp.Rejected):
            function(*args)

    def test_retained_references_have_explicit_noncertification(self):
        report = cmp.compare(self.root, self.pack, self.candidate())
        self.assertTrue(report["passed"])
        self.assertFalse(report["certified"])
        self.assertEqual(report["qualification"], "retained_unqualified")
        self.assertEqual(report["counts"], {"passed": 4})

    def test_numerical_error_is_not_convention_mismatch(self):
        candidate = self.candidate()
        candidate["cases"][0]["metrics"]["year_fraction"]["value"] = "0.9"
        report = cmp.compare(self.root, self.pack, candidate)
        self.assertFalse(report["passed"])
        self.assertEqual(report["counts"]["implementation_error"], 1)

    def convention_exception(self):
        artifact = self.root / "independent-invariant.txt"
        artifact.write_text("For the 182-day period, ACT/360 is 182/360; ACT/365F is 182/365.\n")
        return {"id": "TEST-CONVENTION", "case_id": self.pack["cases"][0]["id"], "differing_fields": ["day_count"],
                "reference_values": {"day_count": "Act360"}, "candidate_values": {"day_count": "Act365Fixed"},
                "owner": "comparison test", "reason": "Independent arithmetic identifies the differing denominator.",
                "reviewed_on": "2026-10-04", "expires_on": "2026-12-31", "tracking_task": "CVX-040",
                "evidence": {"kind": "analytic_invariant", "artifact": {"path": artifact.name, "sha256": cmp.file_sha(artifact),
                    "role": "independent arithmetic"}, "explanation": "Exact elapsed-day invariant; not an external-engine certificate."}}

    def test_convention_difference_requires_evidence_and_never_passes(self):
        candidate = self.candidate()
        candidate["cases"][0]["conventions"]["day_count"] = "Act365Fixed"
        candidate["cases"][0]["metrics"]["year_fraction"]["value"] = str(Decimal(182) / Decimal(365))
        report = cmp.compare(self.root, self.pack, candidate)
        self.assertEqual(report["counts"]["unexplained_convention_mismatch"], 2)
        self.pack["exceptions"] = [self.convention_exception()]
        report = cmp.compare(self.root, self.pack, candidate)
        self.assertEqual(report["counts"]["convention_mismatch"], 2)
        self.assertFalse(report["passed"])
        self.assertFalse(report["certified"])

    def test_wrong_candidate_convention_cannot_reuse_exception(self):
        self.pack["exceptions"] = [self.convention_exception()]
        candidate = self.candidate()
        candidate["cases"][0]["conventions"]["day_count"] = "Thirty360US"
        report = cmp.compare(self.root, self.pack, candidate)
        self.assertEqual(report["counts"]["unexplained_convention_mismatch"], 2)

    def test_exception_expiry_scope_and_artifact_are_enforced(self):
        cases = {row["id"]: row for row in self.pack["cases"]}
        row = self.convention_exception()
        cmp.check_exception(self.root, row, cases, TODAY)
        for field, value in [("expires_on", "2026-10-04"), ("expires_on", "2027-01-04"),
                             ("owner", ""), ("case_id", "unknown")]:
            candidate = copy.deepcopy(row)
            candidate[field] = value
            self.rejects(cmp.check_exception, self.root, candidate, cases, TODAY)
        row["reference_values"]["day_count"] = "ActActIsda"
        self.rejects(cmp.check_exception, self.root, row, cases, TODAY)
        row = self.convention_exception()
        row["evidence"]["artifact"]["sha256"] = "0" * 64
        self.rejects(cmp.check_exception, self.root, row, cases, TODAY)

    def test_missing_case_and_unsupported_are_failed_coverage(self):
        candidate = self.candidate()
        candidate["cases"].pop()
        report = cmp.compare(self.root, self.pack, candidate)
        self.assertEqual(report["counts"]["missing_output"], 1)
        self.assertFalse(report["passed"])
        candidate = self.candidate()
        candidate["cases"][0].update(status="unsupported", metrics={}, error="missing operation")
        report = cmp.compare(self.root, self.pack, candidate)
        self.assertEqual(report["counts"]["unsupported"], 1)
        self.assertFalse(report["passed"])

    def test_failure_cannot_emit_zero_success(self):
        candidate = self.candidate()
        candidate["cases"][0].update(status="error", error="calculation failed")
        self.rejects(cmp.compare, self.root, self.pack, candidate)

    def test_wrong_units_context_extra_rows_and_missing_metrics_fail(self):
        for mutation in ("unit", "context", "extra", "missing", "duplicate"):
            candidate = self.candidate()
            if mutation == "unit":
                candidate["cases"][0]["metrics"]["year_fraction"]["unit"] = "basis_points"
            elif mutation == "context":
                candidate["cases"][0]["context_hash"] = "0" * 64
            elif mutation == "extra":
                candidate["cases"].append({**candidate["cases"][0], "id": "unexpected"})
            elif mutation == "missing":
                candidate["cases"][0]["metrics"].pop("day_count")
            else:
                candidate["cases"].append(candidate["cases"][0])
            with self.subTest(mutation=mutation):
                self.rejects(cmp.compare, self.root, self.pack, candidate)

    def test_nonfinite_malformed_unknown_and_duplicate_json_fail(self):
        for content in (b'{"x":NaN}', b'{"x":Infinity}', b'{"x":1,"x":2}'):
            self.rejects(cmp.decode, content)
        for value in ("NaN", "Infinity", True, "not-a-number", "1e100"):
            self.rejects(cmp.number, value)
        candidate = self.candidate()
        candidate["cases"][0]["metrics"]["year_fraction"]["value"] = "NaN"
        self.rejects(cmp.compare, self.root, self.pack, candidate)
        candidate = self.candidate()
        candidate["unknown_flag"] = True
        self.rejects(cmp.compare, self.root, self.pack, candidate)

    def test_absolute_plus_relative_threshold_and_sign(self):
        rng = random.Random(40040)
        row = self.pack["cases"][0]["metrics"]["year_fraction"]
        row["absolute_tolerance"] = "0.000001"
        row["relative_tolerance"] = "0.000001"
        for index in range(64):
            candidate = self.candidate()
            original = Decimal(candidate["cases"][0]["metrics"]["year_fraction"]["value"])
            threshold = Decimal(row["absolute_tolerance"]) + Decimal(row["relative_tolerance"]) * abs(original)
            factor = Decimal("0.99") if index % 2 else Decimal("1.01")
            delta = threshold * factor * rng.choice((-1, 1))
            candidate["cases"][0]["metrics"]["year_fraction"]["value"] = str(original + delta)
            report = cmp.compare(self.root, self.pack, candidate)
            self.assertEqual(report["passed"], index % 2 == 1)

    def test_future_known_quote_curve_fixing_calendar_rejected(self):
        for name in ("quotes", "curves", "fixings", "calendars"):
            row = copy.deepcopy(self.pack["cases"][0]["context"])
            row[name] = [{"id": "test", "version": "1", "available_at": "2025-01-01T00:00:00Z",
                          "effective_at": row["valuation_at"], "value": {"rate": "0.05"}}]
            self.rejects(cmp.check_context, row)
        row = copy.deepcopy(self.pack["cases"][0]["context"])
        row["known_at"] = "2025-01-01T00:00:00Z"
        self.rejects(cmp.check_context, row)
        row["known_at"] = "2024-01-01T00:00:00"
        self.rejects(cmp.check_context, row)

    def test_pin_missing_artifact_and_path_escape_rejected(self):
        row = copy.deepcopy(self.pack["artifacts"]["reference"])
        row["sha256"] = "0" * 64
        self.rejects(cmp.pin, self.root, row)
        for name in ("missing.json", "../reference.json", "/private/tmp/reference.json"):
            row["path"] = name
            self.rejects(cmp.pin, self.root, row)
        (self.root / "escape.json").symlink_to(ROOT / "Cargo.toml")
        row["path"] = "escape.json"
        self.rejects(cmp.pin, self.root, row)

    def test_unknown_reference_and_negative_tolerance_rejected(self):
        row = self.pack["cases"][0]["metrics"]["year_fraction"]
        row["absolute_tolerance"] = "-1"
        self.rejects(cmp.check_pack, self.root, self.pack)
        row["absolute_tolerance"] = "0"
        row["reference"]["artifact"] = "missing"
        self.rejects(cmp.check_pack, self.root, self.pack)

    def test_capture_cannot_be_promoted_without_runtime_and_license(self):
        catalog = cmp.read(ROOT / cmp.CATALOG)
        read = cmp.read
        with patch.object(cmp, "read", side_effect=lambda path: catalog if str(path).endswith(cmp.CATALOG) else read(path)), patch.object(cmp, "pin", return_value=ROOT / "Cargo.toml"):
            catalog["comparators"][1]["runtime_capture"]["status"] = "captured"
            self.rejects(cmp.check_catalog, ROOT)

    def test_ore_csv_preserves_signed_pv_and_currency(self):
        raw = b"#TradeId,TradeType,NPV,NPVCurrency\nswap-1,Swap,-1234.56,USD\n"
        mapping = {"npv": {"column": "NPV", "unit": "currency", "currency": "USD"}}
        result = cmp.normalize_csv(raw, "ore-npv", mapping)
        self.assertEqual(result["metrics"]["swap-1"]["npv"]["value"], "-1234.56")
        self.assertEqual(result["raw_sha256"], cmp.sha(raw))
        self.assertEqual(result["qualification"], "normalization_only")
        mapping["npv"]["currency"] = "EUR"
        self.rejects(cmp.normalize_csv, raw, "ore-npv", mapping)

    def test_isda_export_never_scales_price_or_upfront_implicitly(self):
        raw = b"case_id,clean_upfront,currency\ncds-1,-0.01234,USD\n"
        mapping = {"clean_upfront": {"column": "clean_upfront", "unit": "fraction", "currency": "USD"}}
        result = cmp.normalize_csv(raw, "isda-export", mapping)
        self.assertEqual(result["metrics"]["cds-1"]["clean_upfront"]["value"], "-0.01234")

    def test_external_csv_duplicate_empty_malformed_and_nonfinite_rejected(self):
        mapping = {"npv": {"column": "NPV", "unit": "currency", "currency": "USD"}}
        for raw in (b"#TradeId,NPV,NPV\nx,1,2\n", b"#TradeId,NPV,NPVCurrency\n",
                    b"#TradeId,NPV,NPVCurrency\nx,NaN,USD\n", b"#TradeId,NPV,NPVCurrency\nx,1\n",
                    b"#TradeId,NPV,NPVCurrency\nx,1,USD\nx,2,USD\n"):
            self.rejects(cmp.normalize_csv, raw, "ore-npv", mapping)

    def base_guard(self, old, new, old_catalog=None, new_catalog=None):
        old_catalog = old_catalog or {"version":1, "packs": [{"path": "pack.json"}], "pending_product_fixtures": []}
        new_catalog = new_catalog or old_catalog
        (self.root / "pack.json").write_text(json.dumps(new))
        sources = {cmp.CATALOG: old_catalog, "pack.json": old}
        with patch.object(cmp, "git_source", side_effect=lambda root, base, path: sources.get(path)), patch.object(cmp.subprocess, "run"):
            return cmp.check_base(self.root, "base", new_catalog)

    def test_tolerance_widening_rejected_after_hash_and_version_refresh(self):
        for field in ("absolute_tolerance", "relative_tolerance"):
            candidate = copy.deepcopy(self.pack)
            candidate["version"] += 1
            candidate["cases"][0]["metrics"]["year_fraction"][field] = "0.1"
            self.rejects(self.base_guard, self.pack, candidate)

    def test_case_metric_unit_operation_and_pack_removal_rejected(self):
        for mutation in ("case", "metric", "unit", "operation", "coverage", "pack", "pending"):
            candidate = copy.deepcopy(self.pack)
            candidate["version"] += 1
            catalog = {"version":2, "packs": [{"path": "pack.json"}], "pending_product_fixtures": []}
            old_catalog = copy.deepcopy(catalog)
            old_catalog['version'] = 1
            if mutation == "case":
                candidate["cases"].pop()
            elif mutation == "metric":
                candidate["cases"][0]["metrics"].pop("year_fraction")
            elif mutation == "unit":
                candidate["cases"][0]["metrics"]["year_fraction"]["unit"] = "basis_points"
            elif mutation == "operation":
                candidate["cases"][0]["operation"] = "different"
            elif mutation == "coverage":
                candidate["retained_unexecuted"].pop()
            elif mutation == "pack":
                catalog["packs"] = []
            else:
                old_catalog["pending_product_fixtures"] = [{"id": "required-product", "comparator": "unavailable"}]
            with self.subTest(mutation=mutation):
                self.rejects(self.base_guard, self.pack, candidate, old_catalog, catalog)

    def test_fixture_edit_requires_version_increment(self):
        candidate = copy.deepcopy(self.pack)
        candidate["cases"][0]["terms"]["end_date"] = "2024-07-16"
        self.rejects(self.base_guard, self.pack, candidate)

    def test_risk_bump_unit_method_and_sign_are_required(self):
        row = self.pack['cases'][0]['metrics']['year_fraction']
        row['bump'] = {'factor':'USD.SOFR.1Y', 'size':'1', 'unit':'basis_points', 'method':'central', 'rebuild':True, 'sign':'PV_down_minus_PV_up'}
        cmp.check_pack(self.root, self.pack)
        for field,value in [('size','0'),('unit','percent'),('method','unspecified'),('rebuild','yes'),('sign','')]:
            candidate = copy.deepcopy(self.pack)
            candidate['cases'][0]['metrics']['year_fraction']['bump'][field] = value
            self.rejects(cmp.check_pack, self.root, candidate)
        row['unit'] = 'currency_per_basis_point'
        row['bump'] = None
        self.rejects(cmp.check_pack, self.root, self.pack)

    def test_tightening_is_allowed_only_by_guard_not_review_approval(self):
        candidate = copy.deepcopy(self.pack)
        candidate["version"] += 1
        candidate["cases"][0]["metrics"]["year_fraction"]["absolute_tolerance"] = "1e-12"
        self.assertEqual(self.base_guard(self.pack, candidate)["existing_packs"], 1)
        # Assurance separately demands the independent quantitative impact record.

    def test_process_failure_and_bad_json_output_cannot_pass(self):
        for output in ("raise SystemExit(7)", "print('not-json')"):
            binary = self.root / "candidate.py"
            binary.write_text(f"#!{sys.executable}\n{output}\n")
            binary.chmod(0o700)
            with self.assertRaises((cmp.Rejected, ValueError)):
                cmp.execute(binary, self.pack)

    def test_python_optimization_keeps_rejections_enabled(self):
        script = f"import sys; sys.path.insert(0,{str(ROOT / 'reconciliation')!r}); import comparison; comparison.number('NaN')"
        result = subprocess.run([sys.executable, "-O", "-c", script], capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"Rejected", result.stderr)

    def test_stale_pass_report_is_replaced_on_candidate_validation_failure(self):
        candidate = self.root / "invalid.json"
        candidate.write_text('{"schema_version":99}')
        report = self.root / "report.json"
        report.write_text('{"passed":true}')
        result = subprocess.run([sys.executable, str(ROOT / 'reconciliation/comparison.py'), 'run',
                                 '--candidate-results', str(candidate), '--report', str(report)], capture_output=True)
        self.assertEqual(result.returncode, 2)
        output = cmp.read(report)
        self.assertFalse(output['passed'])
        self.assertFalse(output['certified'])

    def test_catalog_cannot_silently_repin_a_changed_tolerance(self):
        catalog = cmp.read(ROOT / cmp.CATALOG)
        altered = copy.deepcopy(catalog)
        altered['packs'][0]['sha256'] = '0' * 64
        read = cmp.read
        with patch.object(cmp, 'read', side_effect=lambda path: altered if str(path).endswith(cmp.CATALOG) else read(path)):
            self.rejects(cmp.check_catalog, ROOT)


if __name__ == "__main__":
    unittest.main()
