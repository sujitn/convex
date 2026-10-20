"""Invalid scope fails cheaply; valid source evidence is fresh on every call."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("validation_order", Path(__file__).with_name("check.py"))
coverage = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(coverage)


class ValidationOrderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.document = coverage.decode((coverage.ROOT / coverage.MANIFEST).read_bytes())

    def test_invalid_mandatory_metadata_fails_before_source_reads(self):
        for field, value in (("status", "Verified"), ("mandatory", False),
                             ("surface", "invented_surface")):
            candidate = copy.deepcopy(self.document)
            candidate["records"][0][field] = value
            with self.subTest(field=field), patch.object(
                coverage, "file_path", side_effect=AssertionError("invalid scope caused source I/O")
            ), self.assertRaises(coverage.Failure):
                coverage.validate(candidate)

    def test_valid_metadata_reads_every_inspected_source(self):
        original = coverage.file_path
        seen = []

        def observed(root, path):
            seen.append(path)
            return original(root, path)

        with patch.object(coverage, "file_path", side_effect=observed):
            coverage.validate(self.document)
        self.assertEqual(len(seen), len(self.document["inspected_sources"]))
        self.assertEqual(set(seen), set(self.document["inspected_sources"]))

    def test_changed_source_between_calls_cannot_use_cached_evidence(self):
        selected = "crates/convex-core/src/cashflows/profiles.rs"
        original = coverage.file_path
        with tempfile.TemporaryDirectory() as directory:
            shadow = Path(directory) / "source.rs"
            shadow.write_bytes(original(coverage.ROOT, selected).read_bytes())

            def supplied(root, path):
                return shadow if path == selected else original(root, path)

            with patch.object(coverage, "file_path", side_effect=supplied):
                coverage.validate(self.document)
                shadow.write_bytes(b"// anchors removed\n")
                with self.assertRaisesRegex(coverage.Failure, "missing inspected anchor"):
                    coverage.validate(self.document)


if __name__ == "__main__":
    unittest.main()
