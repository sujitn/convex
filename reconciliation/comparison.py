#!/usr/bin/env python3
"""Offline quantitative comparisons. Python 3.11+, standard library only.

This is a test tool, never a dependency or data provider of production pricing.
No download, regeneration, tolerance refresh or failure waiver is implemented.
"""
from __future__ import annotations

import argparse
import csv
import datetime as dt
from decimal import Decimal, InvalidOperation
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CATALOG = "tests/fixtures/comparisons/catalog.json"
POLICY_PINS = {CATALOG, "reconciliation/comparison.py", "crates/convex-analytics/examples/offline_compare.rs"}
MAX_BYTES = 8 * 1024 * 1024
MAX_CASES = 2048
MAX_METRICS = 64
UNITS = {"days", "years", "years_squared", "fraction", "rate_decimal", "basis_points",
         "price_per_100", "currency", "currency_per_basis_point", "price_points"}


class Rejected(ValueError):
    """Explicit validation failure, including under python -O."""


def require(condition, message):
    if not condition:
        raise Rejected(message)


def sha(content):
    return hashlib.sha256(content).hexdigest()


def file_sha(path):
    result = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(65536), b""):
            result.update(block)
    return result.hexdigest()


def canonical(document):
    return json.dumps(document, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def decode(content):
    require(len(content) <= MAX_BYTES, "JSON exceeds 8 MiB")
    return json.loads(content, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(Rejected(f"nonfinite JSON: {value}")))


def read(path):
    with path.open("rb") as handle:
        content = handle.read(MAX_BYTES + 1)
    return decode(content)


def fields(document, required, optional=()):
    require(isinstance(document, dict) and set(required) <= set(document)
            and set(document) <= set(required) | set(optional), f"invalid fields; expected {sorted(required)}")


def text(value, label):
    require(isinstance(value, str) and 0 < len(value) <= 4096 and value.strip(), f"invalid {label}")
    return value


def number(value):
    require(isinstance(value, (str, int, float)) and not isinstance(value, bool), "metric must be numeric")
    try:
        result = Decimal(str(value))
    except InvalidOperation as error:
        raise Rejected("invalid decimal metric") from error
    require(result.is_finite() and abs(result) <= Decimal("1e24"), "nonfinite/oversized metric")
    return result


def timestamp(value):
    instant = dt.datetime.fromisoformat(text(value, "timestamp").replace("Z", "+00:00"))
    require(instant.tzinfo is not None and instant.utcoffset() == dt.timedelta(0), "timestamp must be UTC")
    return instant


def local_path(root, name):
    text(name, "artifact path")
    path = Path(name)
    require(not path.is_absolute() and ".." not in path.parts, "artifact must have a repository-relative path")
    resolved = (root / path).resolve()
    require(resolved.is_relative_to(root.resolve()), "artifact symlink escapes repository")
    require(resolved.is_file(), f"missing artifact: {name}")
    return resolved


def pin(root, row):
    fields(row, {"path", "sha256", "role"})
    require(re.fullmatch(r"[0-9a-f]{64}", row["sha256"]) is not None, "invalid SHA-256")
    path = local_path(root, row["path"])
    require(0 < path.stat().st_size <= 512 * 1024 * 1024, f"empty/artifact exceeds 512 MiB: {row['path']}")
    require(file_sha(path) == row["sha256"], f"artifact pin changed: {row['path']}")
    text(row["role"], "artifact role")
    return path


def pointer(document, value):
    require(isinstance(value, str) and value.startswith("/") and len(value) <= 1024, "invalid JSON pointer")
    for segment in value[1:].split("/"):
        key = segment.replace("~1", "/").replace("~0", "~")
        document = document[int(key)] if isinstance(document, list) else document[key]
    return document


def context(case):
    return sha(canonical({k: case[k] for k in ("id", "operation", "terms", "conventions", "context", "model")}))


def check_context(row):
    fields(row, {"valuation_at", "settlement_date", "known_at", "dataset_kind", "reporting_currency",
                 "reference_versions", "curves", "quotes", "fixings", "calendars", "rounding"})
    valuation = timestamp(row["valuation_at"])
    cutoff = timestamp(row["known_at"])
    require(cutoff <= valuation, "known-at cutoff is later than valuation")
    dt.date.fromisoformat(row["settlement_date"])
    require(row["dataset_kind"] in {"synthetic", "historical"}, "unknown data kind")
    require(re.fullmatch(r"[A-Z]{3}", row["reporting_currency"]) is not None, "missing currency")
    require(isinstance(row["reference_versions"], dict) and row["reference_versions"], "missing reference versions")
    for key, value in row["reference_versions"].items():
        text(key, "reference name")
        text(value, "reference version")
    require(isinstance(row["rounding"], dict) and row["rounding"], "missing rounding policy")
    for name in ("curves", "quotes", "fixings", "calendars"):
        require(isinstance(row[name], list) and len(row[name]) <= 4096, f"unbounded {name}")
        for observation in row[name]:
            fields(observation, {"id", "version", "available_at", "effective_at", "value"})
            text(observation["id"], "observation ID")
            text(observation["version"], "observation version")
            require(timestamp(observation["available_at"]) <= cutoff, f"later-known {name} input")
            require(timestamp(observation["effective_at"]) <= valuation, f"future effective {name} input")


def check_catalog(root):
    policy = read(root / "tools/assurance/numeric-evidence.json")
    pins = {row["path"]: row for row in policy["artifacts"]}
    require(POLICY_PINS <= set(pins), "comparison policy pins missing")
    for name in POLICY_PINS:
        require(pins[name]["mode"] == "file" and file_sha(local_path(root, name)) == pins[name]["sha256"],
                f"comparison policy pin changed: {name}")
    document = read(root / CATALOG)
    fields(document, {"schema_version", "version", "comparators", "packs", "pending_product_fixtures"})
    require(document["schema_version"] == 1 and type(document["version"]) is int and document["version"] > 0,
            "invalid catalog version")
    require(0 < len(document["comparators"]) <= 16 and 0 < len(document["packs"]) <= 64
            and len(document["pending_product_fixtures"]) <= 64, "unbounded comparator catalog")
    artifacts = document["packs"] + [row["template"] for row in document["pending_product_fixtures"]]
    artifacts += [row["license_record"] for row in document["comparators"]]
    for artifact in artifacts:
        require(artifact["path"] in pins and pins[artifact["path"]]["mode"] == "file"
                and pins[artifact["path"]]["sha256"] == artifact["sha256"], "fixture catalog differs from reviewed quantitative pins")
    identifiers = set()
    for row in document["comparators"]:
        fields(row, {"id", "family", "version", "source_revision", "source_url", "license", "license_url",
                     "license_record", "runtime_capture", "limitations"})
        require(row["id"] not in identifiers, "duplicate comparator")
        identifiers.add(row["id"])
        require(row["family"] in {"QuantLib", "ORE", "ISDA", "exchange"}, "unknown comparator family")
        for key in ("id", "version", "source_revision", "source_url", "license", "license_url", "limitations"):
            text(row[key], key)
        pin(root, row["license_record"])
        capture = row["runtime_capture"]
        fields(capture, {"status", "source", "binary", "execution", "toolchain", "dependencies", "arguments", "environment", "platform"})
        require(capture["status"] in {"retained_unqualified", "not_executed", "document_only", "captured"},
                "unknown capture status")
        for key in ("source", "binary", "execution"):
            if capture[key] is not None:
                pin(root, capture[key])
        if capture["status"] == "captured":
            for key in ("source", "binary", "execution"):
                require(capture[key] is not None and capture[key]["role"] == f"comparator_{key}", f"missing {key} capture")
            require(len({capture[key]["path"] for key in ("source", "binary", "execution")}) == 3, "capture artifacts must be distinct")
            require(row["license_record"]["role"] == "comparator_license", "full captured license/attribution artifact required")
            for key in ("toolchain", "dependencies", "environment"):
                require(isinstance(capture[key], dict) and capture[key], f"missing capture {key}")
            fields(capture["toolchain"], {"name", "version", "flags"})
            text(capture["toolchain"]["name"], "compiler name")
            text(capture["toolchain"]["version"], "compiler version")
            require(isinstance(capture["toolchain"]["flags"], list) and len(capture["toolchain"]["flags"]) <= 256
                    and all(isinstance(flag, str) for flag in capture["toolchain"]["flags"]), "invalid compiler flags")
            for key in ("dependencies", "environment"):
                for name, value in capture[key].items():
                    text(name, f"{key} name")
                    text(value, f"{key} value/version")
            require(isinstance(capture["arguments"], list) and capture["arguments"]
                    and all(isinstance(arg, str) and len(arg) <= 4096 for arg in capture["arguments"]), "missing capture arguments")
            text(capture["platform"], "capture platform")
    paths = set()
    require(document["packs"], "empty comparison catalog")
    for row in document["packs"]:
        path = pin(root, row)
        require(row["path"] not in paths, "duplicate fixture pack")
        paths.add(row["path"])
        pack = check_pack(root, read(path), identifiers)
        comparator = next(row for row in document["comparators"] if row["id"] == pack["comparator"])
        require(pack["qualification"] == comparator["runtime_capture"]["status"], "pack qualification exceeds comparator capture")
    pending_ids = set()
    for row in document["pending_product_fixtures"]:
        fields(row, {"id", "comparator", "owning_tasks", "reason", "template"})
        require(row["id"] not in pending_ids and row["comparator"] in identifiers, "invalid pending fixture")
        pending_ids.add(row["id"])
        require(row["owning_tasks"] and row["reason"], "pending fixture needs owner and reason")
        template = read(pin(root, row["template"]))
        require(template["state"] == "pending_product_fixture" and template["expected_outputs"] is None,
                "pending templates must not contain fabricated comparison outputs")
    return document


def check_exception(root, row, cases, today):
    fields(row, {"id", "case_id", "differing_fields", "reference_values", "candidate_values", "owner",
                 "reason", "reviewed_on", "expires_on", "tracking_task", "evidence"})
    require(row["case_id"] in cases, "exception has unknown case")
    for key in ("id", "owner", "reason", "tracking_task"):
        text(row[key], key)
    reviewed = dt.date.fromisoformat(row["reviewed_on"])
    expires = dt.date.fromisoformat(row["expires_on"])
    require(reviewed <= today < expires and 0 < (expires-reviewed).days <= 90, "expired/future/unbounded exception")
    keys = row["differing_fields"]
    require(keys and len(keys) == len(set(keys)) and set(keys) == set(row["reference_values"]) == set(row["candidate_values"]),
            "exception convention scope is incomplete")
    case = cases[row["case_id"]]
    for key in keys:
        require(key in case["conventions"] and case["conventions"][key] == row["reference_values"][key]
                and row["reference_values"][key] != row["candidate_values"][key], "stale convention evidence")
    fields(row["evidence"], {"kind", "artifact", "explanation"})
    require(row["evidence"]["kind"] in {"analytic_invariant", "second_independent_source"}, "exception needs independent evidence")
    pin(root, row["evidence"]["artifact"])
    text(row["evidence"]["explanation"], "evidence explanation")


def check_pack(root, document, comparators=None, today=None):
    fields(document, {"schema_version", "id", "version", "comparator", "qualification", "qualification_limits",
                     "artifacts", "cases", "exceptions", "retained_unexecuted"})
    require(document["schema_version"] == 1 and type(document["version"]) is int and document["version"] > 0,
            "invalid pack version")
    if comparators is not None:
        require(document["comparator"] in comparators, "unknown comparator")
    require(document["qualification"] in {"retained_unqualified", "captured", "document_only"}, "unknown qualification")
    text(document["qualification_limits"], "qualification limits")
    require(isinstance(document["artifacts"], dict) and 0 < len(document["artifacts"]) <= 128,
            "missing/unbounded artifacts")
    paths = {}
    for identifier, artifact in document["artifacts"].items():
        paths[identifier] = pin(root, artifact)
    require(paths and 0 < len(document["cases"]) <= MAX_CASES, "missing/unbounded cases")
    expected = {}
    cases = {}
    for case in document["cases"]:
        fields(case, {"id", "operation", "terms", "conventions", "context", "model", "metrics", "deferred_metrics"})
        text(case["id"], "case ID")
        require(case["id"] not in cases, "duplicate fixture case")
        cases[case["id"]] = case
        text(case["operation"], "operation")
        require(isinstance(case["terms"], dict) and case["terms"], "missing economics")
        require(isinstance(case["conventions"], dict) and case["conventions"], "missing conventions")
        fields(case["model"], {"name", "version", "settings", "approximations"})
        text(case["model"]["name"], "model")
        text(case["model"]["version"], "model version")
        require(isinstance(case["model"]["settings"], dict) and case["model"]["settings"], "missing numerical settings")
        require(isinstance(case["model"]["approximations"], list), "approximations must be explicit")
        check_context(case["context"])
        require(0 < len(case["metrics"]) <= MAX_METRICS, "missing/unbounded metrics")
        for metric, row in case["metrics"].items():
            text(metric, "metric")
            fields(row, {"unit", "absolute_tolerance", "relative_tolerance", "justification", "reference", "bump"})
            require(row["unit"] in UNITS, "unknown metric unit")
            require(number(row["absolute_tolerance"]) >= 0 and number(row["relative_tolerance"]) >= 0,
                    "negative tolerance")
            text(row["justification"], "tolerance justification")
            require(row["bump"] is None or isinstance(row["bump"], dict) and row["bump"], "missing bump policy")
            if row["bump"] is not None:
                fields(row["bump"], {"factor", "size", "unit", "method", "rebuild", "sign"})
                text(row["bump"]["factor"], "risk factor")
                text(row["bump"]["sign"], "sensitivity sign definition")
                require(number(row["bump"]["size"]) > 0 and row["bump"]["unit"] in UNITS
                        and row["bump"]["method"] in {"central", "forward", "backward"}
                        and type(row["bump"]["rebuild"]) is bool, "invalid bump/rebuild method")
            require(row["unit"] != "currency_per_basis_point" or row["bump"] is not None, "monetary sensitivity needs a bump definition")
            fields(row["reference"], {"artifact", "pointer"})
            identifier = row["reference"]["artifact"]
            require(identifier in paths, "unknown expected artifact")
            if identifier not in expected:
                expected[identifier] = read(paths[identifier])
            number(pointer(expected[identifier], row["reference"]["pointer"]))
        for metric, reason in case["deferred_metrics"].items():
            require(metric not in case["metrics"], "metric both compared and deferred")
            text(reason, "deferred metric reason")
    require(isinstance(document["retained_unexecuted"], list), "unexecuted coverage must be explicit")
    require(len(document["retained_unexecuted"]) <= 64 and len(document["exceptions"]) <= MAX_CASES,
            "unbounded exceptions/retained coverage")
    identifiers = set()
    for row in document["exceptions"]:
        require(row["id"] not in identifiers, "duplicate exception")
        identifiers.add(row["id"])
        check_exception(root, row, cases, today or dt.datetime.now(dt.timezone.utc).date())
    return document


def compare(root, pack, candidate):
    check_pack(root, pack)
    fields(candidate, {"schema_version", "producer", "cases"})
    require(candidate["schema_version"] == 1, "unknown output schema")
    fields(candidate["producer"], {"name", "version", "approximations"})
    text(candidate["producer"]["name"], "producer")
    text(candidate["producer"]["version"], "producer version")
    require(isinstance(candidate["producer"]["approximations"], list), "candidate approximations missing")
    require(isinstance(candidate["cases"], list) and len(candidate["cases"]) <= MAX_CASES, "unbounded candidate")
    observed = {}
    for row in candidate["cases"]:
        fields(row, {"id", "context_hash", "status", "conventions", "metrics", "error"})
        require(row["id"] not in observed, "duplicate output case")
        require(row["status"] in {"ok", "unsupported", "error"}, "unknown candidate status")
        observed[row["id"]] = row
    expected_ids = {case["id"] for case in pack["cases"]}
    require(set(observed) <= expected_ids, "unexpected output cases")
    artifacts = {key: read(local_path(root, row["path"])) for key, row in pack["artifacts"].items()
                 if any(metric["reference"]["artifact"] == key for case in pack["cases"] for metric in case["metrics"].values())}
    rows = []
    for case in pack["cases"]:
        actual = observed.get(case["id"])
        if actual is None:
            rows.append({"case_id": case["id"], "classification": "missing_output", "metric": None})
            continue
        require(actual["context_hash"] == context(case), "wrong/mixed candidate context")
        if actual["status"] != "ok":
            require(actual["error"] and not actual["metrics"], "failed output must not contain plausible values")
            rows.append({"case_id": case["id"], "classification": "unsupported" if actual["status"] == "unsupported" else "calculation_error",
                         "metric": None, "error": actual["error"]})
            continue
        require(actual["error"] is None and isinstance(actual["conventions"], dict), "invalid successful output")
        require(set(actual["metrics"]) == set(case["metrics"]), "missing/unexpected output metrics")
        differences = {key for key in set(case["conventions"]) | set(actual["conventions"])
                       if case["conventions"].get(key) != actual["conventions"].get(key)}
        evidence = [row for row in pack["exceptions"] if row["case_id"] == case["id"]
                    and set(row["differing_fields"]) == differences
                    and all(actual["conventions"].get(key) == row["candidate_values"][key] for key in differences)]
        for metric, definition in case["metrics"].items():
            measured = actual["metrics"][metric]
            fields(measured, {"value", "unit"})
            require(measured["unit"] == definition["unit"], "metric units differ; no implicit conversion")
            reference = definition["reference"]
            target = number(pointer(artifacts[reference["artifact"]], reference["pointer"]))
            value = number(measured["value"])
            tolerance = number(definition["absolute_tolerance"]) + number(definition["relative_tolerance"]) * abs(target)
            delta = value - target
            classification = "passed" if abs(delta) <= tolerance else "implementation_error"
            if differences:
                classification = "convention_mismatch" if evidence else "unexplained_convention_mismatch"
            rows.append({"case_id": case["id"], "metric": metric, "unit": definition["unit"], "expected": str(target),
                         "actual": str(value), "delta": str(delta), "tolerance": str(tolerance), "classification": classification,
                         "differing_fields": sorted(differences), "evidence": [row["id"] for row in evidence]})
    counts = {}
    for row in rows:
        label = row["classification"]
        counts[label] = counts.get(label, 0) + 1
    return {"schema_version": 1, "pack_id": pack["id"], "pack_version": pack["version"],
            "pack_content_sha256": sha(canonical(pack)), "comparator": pack["comparator"],
            "qualification": pack["qualification"], "qualification_limits": pack["qualification_limits"],
            "candidate_producer": candidate["producer"], "case_contexts": {case["id"]: context(case) for case in pack["cases"]},
            "expected_case_count": len(pack["cases"]), "expected_measure_count": sum(len(case["metrics"]) for case in pack["cases"]),
            "compared_measure_count": sum(row["metric"] is not None for row in rows),
            "counts": counts, "passed": all(row["classification"] == "passed" for row in rows),
            "certified": False, "rows": rows,
            "deferred_metrics": {case["id"]: case["deferred_metrics"] for case in pack["cases"] if case["deferred_metrics"]},
            "retained_unexecuted": pack["retained_unexecuted"]}


def execute(binary, pack):
    request = {"schema_version": 1, "cases": [{"id": case["id"], "operation": case["operation"],
                "terms": case["terms"], "conventions": case["conventions"], "context": case["context"],
                "model": case["model"], "context_hash": context(case)} for case in pack["cases"]]}
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        # A reviewed, local executable is trusted to run; it receives no shell or network command.
        result = subprocess.run([str(binary.resolve())], input=canonical(request), stdout=output, stderr=errors, timeout=30)
        output.seek(0)
        errors.seek(0)
        require(result.returncode == 0, f"candidate process failed ({result.returncode}): {errors.read(8192).decode(errors='replace')}")
        return decode(output.read(MAX_BYTES + 1))


def normalize_csv(content, family, mapping):
    """Normalize a captured ORE NPV or product-exported ISDA grid. Never price."""
    require(len(content) <= MAX_BYTES, "CSV exceeds 8 MiB")
    require(family in {"ore-npv", "isda-export"}, "unknown CSV family")
    reader = csv.DictReader(io.StringIO(content.decode("utf-8-sig")))
    headers = reader.fieldnames or []
    require(headers and len(headers) <= 256 and len(headers) == len(set(headers)), "missing/duplicate/unbounded CSV headers")
    key_column = "#TradeId" if family == "ore-npv" else "case_id"
    require(key_column in headers, "missing external row key")
    require(mapping and len(mapping) <= MAX_METRICS, "missing/unbounded column mapping")
    for metric, rule in mapping.items():
        fields(rule, {"column", "unit", "currency"})
        require(rule["column"] in headers and rule["unit"] in UNITS, "unknown column/unit")
        if rule["unit"] in {"currency", "currency_per_basis_point"}:
            require(rule["currency"] is not None and re.fullmatch(r"[A-Z]{3}", rule["currency"]), "currency is required")
    result = {}
    for row in reader:
        require(None not in row and None not in row.values(), "malformed CSV row")
        identifier = text(row[key_column], "external case ID")
        require(identifier not in result and len(result) < MAX_CASES, "duplicate/unbounded external rows")
        result[identifier] = {}
        for metric, rule in mapping.items():
            if rule["currency"] is not None:
                currency_column = "NPVCurrency" if family == "ore-npv" else "currency"
                require(row.get(currency_column) == rule["currency"], "external currency differs; no FX substitution")
            result[identifier][metric] = {"value": str(number(row[rule["column"]])), "unit": rule["unit"]}
    require(result, "empty external output")
    return {"schema_version": 1, "source_format": family, "raw_sha256": sha(content), "metrics": result,
            "qualification": "normalization_only", "ignored_columns": sorted(set(headers)-{key_column}-
                {rule["column"] for rule in mapping.values()}-{"NPVCurrency", "currency"})}


def git_source(root, base, path):
    result = subprocess.run(["git", "show", f"{base}:{path}"], cwd=root, capture_output=True)
    return decode(result.stdout) if result.returncode == 0 else None


def check_base(root, base, catalog):
    """An existing numerical threshold cannot be relaxed, even after a hash refresh.

    Other edits are protected by tools/assurance/check.py's exact independent
    quantitative impact record. This check adds case/measure conservation.
    """
    subprocess.run(["git", "rev-parse", "--verify", f"{base}^{{commit}}"], cwd=root, check=True, capture_output=True)
    previous = git_source(root, base, CATALOG)
    if previous is None:
        return {"baseline": base, "existing_packs": 0, "note": "initial infrastructure admission"}
    if canonical(previous) != canonical(catalog):
        require(catalog["version"] > previous["version"], "changed catalog needs a version increment")
    old_paths = {row["path"] for row in previous["packs"]}
    new_paths = {row["path"] for row in catalog["packs"]}
    require(old_paths <= new_paths, "existing comparison pack removed")
    pending = {row["id"] for row in catalog["pending_product_fixtures"]}
    promoted = {pack["id"]: pack["comparator"] for pack in (read(root / path) for path in new_paths) if pack["qualification"] == "captured"}
    require(all(row["id"] in pending or promoted.get(row["id"]) == row["comparator"]
                for row in previous["pending_product_fixtures"]), "pending product requirements removed without captured fixtures")
    for path in old_paths:
        old = git_source(root, base, path)
        require(old is not None, "baseline pack is unavailable")
        new = read(root / path)
        if canonical(old) != canonical(new):
            require(new["version"] > old["version"], "changed fixture needs a version increment")
        old_cases = {case["id"]: case for case in old["cases"]}
        new_cases = {case["id"]: case for case in new["cases"]}
        require(set(old_cases) <= set(new_cases), "existing fixture case removed")
        require(all(row in new["retained_unexecuted"] for row in old["retained_unexecuted"]), "unexecuted retained coverage removed")
        for identifier, old_case in old_cases.items():
            require(old_case["operation"] == new_cases[identifier]["operation"], "existing case operation changed")
            metrics = new_cases[identifier]["metrics"]
            require(set(old_case["metrics"]) <= set(metrics), "existing measure removed/deferred")
            for name, old_metric in old_case["metrics"].items():
                require(old_metric["unit"] == metrics[name]["unit"], "existing metric unit changed")
                for field in ("absolute_tolerance", "relative_tolerance"):
                    require(number(metrics[name][field]) <= number(old_metric[field]), f"tolerance widening prohibited: {identifier}/{name}/{field}")
    return {"baseline": base, "existing_packs": len(old_paths)}


def save(path, document):
    content = json.dumps(document, indent=2, sort_keys=True, allow_nan=False) + "\n"
    with tempfile.NamedTemporaryFile(mode="w", dir=path.parent, delete=False) as handle:
        temporary = Path(handle.name)
        handle.write(content)
    try:
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def source_fingerprint(root):
    paths = [root / name for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")]
    for name in ("convex-core", "convex-math", "convex-curves", "convex-bonds", "convex-analytics"):
        paths.append(root / "crates" / name / "Cargo.toml")
        paths.extend((root / "crates" / name / "src").rglob("*.rs"))
    paths.append(root / "crates/convex-analytics/examples/offline_compare.rs")
    return {str(path.relative_to(root)): file_sha(path) for path in sorted(paths)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check")
    check.add_argument("--base")
    sub.add_parser("catalog")
    run = sub.add_parser("run")
    run.add_argument("--pack", default="tests/fixtures/comparisons/quantlib-1.40.json")
    choice = run.add_mutually_exclusive_group(required=True)
    choice.add_argument("--candidate-binary", type=Path)
    choice.add_argument("--candidate-results", type=Path)
    run.add_argument("--report", type=Path, required=True)
    normalize = sub.add_parser("normalize")
    normalize.add_argument("--format", choices=["ore-npv", "isda-export"], required=True)
    normalize.add_argument("--raw", type=Path, required=True)
    normalize.add_argument("--mapping", type=Path, required=True)
    normalize.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        if args.command == "normalize":
            with args.raw.open("rb") as handle:
                result = normalize_csv(handle.read(MAX_BYTES + 1), args.format, read(args.mapping))
            save(args.output, result)
            print(json.dumps({"status": "normalized", "qualification": "normalization_only", "raw_sha256": result["raw_sha256"]}))
            return 0
        catalog = check_catalog(root)
        if args.command in {"check", "catalog"}:
            result = {"status": "passed", "catalog_version": catalog["version"], "packs": len(catalog["packs"]),
                      "pending_product_fixtures": catalog["pending_product_fixtures"], "comparators": catalog["comparators"],
                      "certified": False}
            if args.command == "check" and args.base:
                result["change_guard"] = check_base(root, args.base, catalog)
            print(json.dumps(result, sort_keys=True))
            return 0
        require(args.pack in {row["path"] for row in catalog["packs"]}, "pack is not admitted by catalog")
        pack = read(local_path(root, args.pack))
        input_path = args.candidate_binary or args.candidate_results
        require(args.report.resolve() != input_path.resolve()
                and not args.report.resolve().is_relative_to((root / "tests/fixtures").resolve()), "report would overwrite an input/fixture")
        candidate = execute(input_path, pack) if args.candidate_binary else read(input_path)
        capture = args.report.with_suffix(".candidate.json")
        require(capture.resolve() != input_path.resolve(), "candidate capture would overwrite input")
        save(capture, candidate)
        report = compare(root, pack, candidate)
        report["catalog_content_sha256"] = sha(canonical(catalog))
        report["catalog_file_sha256"] = file_sha(root / CATALOG)
        report["pack_file_sha256"] = file_sha(root / args.pack)
        report["reference_comparator"] = next(row for row in catalog["comparators"] if row["id"] == pack["comparator"])
        report["candidate_artifact"] = {"kind": "executed_binary" if args.candidate_binary else "captured_results",
                                        "path": str(input_path.resolve()), "sha256": file_sha(input_path)}
        report["candidate_source_commit"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        report["candidate_dirty_paths"] = subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True).splitlines()
        report["candidate_source_files"] = source_fingerprint(root)
        report["source_binding"] = "Local source-tree fingerprint; compilation-to-binary binding requires the recorded build receipt. No binary attestation is inferred."
        report["normalized_candidate_capture"] = {"path": str(capture.resolve()), "sha256": file_sha(capture)}
        report["performed_at"] = dt.datetime.now(dt.timezone.utc).isoformat()
        report["artifacts"] = pack["artifacts"]
        save(args.report, report)
        print(json.dumps({key: report[key] for key in ("pack_id", "counts", "passed", "qualification", "certified")}, sort_keys=True))
        return 0 if report["passed"] else 1
    except (Rejected, OSError, ValueError, TypeError, KeyError, IndexError, RecursionError, subprocess.SubprocessError) as error:
        if (args.command == "run" and args.report.resolve() not in {
                (root / args.pack).resolve(), args.candidate_binary.resolve() if args.candidate_binary else args.candidate_results.resolve()}
                and not args.report.resolve().is_relative_to((root / "tests/fixtures").resolve())):
            try:
                save(args.report, {"schema_version": 1, "passed": False, "certified": False,
                                   "classification": "validation_or_process_error", "error": str(error)})
            except OSError as report_error:
                print(f"failed to write rejection report: {report_error}", file=sys.stderr)
        print(f"comparison rejected: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
