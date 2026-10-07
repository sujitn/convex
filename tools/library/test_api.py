"""Capability report mutation checks; no financial certificate is inferred."""
import copy
import json
from pathlib import Path
import unittest
import tempfile

import api

ROOT = Path(__file__).resolve().parents[2]


class ApiMutations(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.matrix = json.loads((ROOT / "tools/library/api-capabilities.json").read_bytes())

    def candidate(self):
        return copy.deepcopy(self.matrix)

    def rejects(self, candidate):
        with self.assertRaises((ValueError, KeyError)):
            api.check(candidate)

    def test_complete_scope_is_retained_and_uncertified(self):
        result = api.check(self.matrix)
        self.assertEqual(result["mandatory_records"], 2154)
        self.assertFalse(result["certified"])
        self.assertEqual(result["verified_records"], 0)
        self.assertGreater(result["unsupported_records"], 0)

    def test_missing_duplicate_or_changed_record_is_rejected(self):
        candidate = self.candidate(); candidate["records"].pop(); self.rejects(candidate)
        candidate = self.candidate(); candidate["records"].append(candidate["records"][0]); self.rejects(candidate)
        candidate = self.candidate(); candidate["records"][0]["surface"] = "server_only"; self.rejects(candidate)
        candidate = self.candidate(); candidate["mandatory_scope_sha256"] = "0" * 64; self.rejects(candidate)

    def test_adapter_inventory_cannot_promote_financial_certificates(self):
        for field, value in (("certified", True), ("verified_records", 1)):
            candidate = self.candidate(); candidate[field] = value; self.rejects(candidate)
        candidate = self.candidate(); candidate["records"][0]["certificate_status"] = "Verified"; self.rejects(candidate)

    def test_route_needs_actual_sources_and_outcomes(self):
        for field, value in (("qualification", ""), ("positive_examples", []),
                             ("negative_examples", ["schedule.independent"]),
                             ("positive_examples", ["invented.fixture"])):
            candidate = self.candidate(); candidate["routes"]["LC-001"][field] = value; self.rejects(candidate)
        candidate = self.candidate()
        candidate["routes"]["LC-001"]["anchors"][0]["text"] = "invented dispatch"
        self.rejects(candidate)
        candidate = self.candidate(); candidate["records"][0]["route_id"] = "LC-002"; self.rejects(candidate)

    def test_required_unsupported_work_cannot_lose_its_owner_or_reason(self):
        for field, value in (("remaining_owner_tasks", []), ("qualification", ""),
                             ("dispatch_status", "complete")):
            candidate = self.candidate()
            row = next(r for r in candidate["records"] if r["dispatch_status"] == "unsupported")
            row[field] = value; self.rejects(candidate)

    def test_entire_client_or_required_gate_cannot_disappear(self):
        candidate = self.candidate(); candidate["clients"].pop("wasm"); self.rejects(candidate)
        candidate = self.candidate()
        # Select a real required surface rather than letting an unknown spelling pass.
        removed = self.matrix["records"][0]["surface"]
        candidate["records"] = [r for r in candidate["records"] if r["surface"] != removed]
        self.rejects(candidate)

    def test_metadata_decoder_rejects_duplicates_nonfinite_and_oversize(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.json"
            for content in ('{"certified":false,"certified":true}', '{"value":NaN}',
                            ' ' * (8 * 1024 * 1024 + 1)):
                path.write_text(content)
                with self.assertRaises(ValueError):
                    api.read(path)


if __name__ == "__main__":
    unittest.main()
