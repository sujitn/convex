#!/usr/bin/env python3
"""Check reviewed library coverage metadata; never infer certification from code."""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = "tools/library/library-inventory.json"
SURFACES = {"rust", "shared_json", "c_abi", "java_advertised",
            "excel_advertised", "mcp_advertised", "wasm_advertised"}
PROFILES = ["rust-default", "rust-no-default", "rust-schemars"]
ASSESSMENTS = {"partial", "missing", "type_only", "missing_dispatch",
               "approximation", "candidate"}


class Failure(ValueError):
    """A failed requirement; validation remains active under python -O."""


def require(condition, message):
    if not condition:
        raise Failure(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"duplicate JSON key: {key}")
        result[key] = value
    return result


def decode(content):
    require(len(content) <= 8 * 1024 * 1024, "inventory exceeds 8 MiB")
    def nonfinite(value):
        raise Failure(f"nonfinite JSON number: {value}")
    return json.loads(content, object_pairs_hook=unique_object, parse_constant=nonfinite)


def file_path(root, path):
    require(isinstance(path, str) and path and not Path(path).is_absolute()
            and ".." not in Path(path).parts, f"unsafe evidence path: {path}")
    resolved = (root / path).resolve()
    require(resolved.is_relative_to(root.resolve()), f"evidence escapes root: {path}")
    require(resolved.is_file(), f"missing evidence file: {path}")
    return resolved


def keyed(rows, label):
    require(isinstance(rows, list) and 0 < len(rows) <= 32768,
            f"{label} must be a bounded nonempty list")
    result = {}
    for row in rows:
        require(isinstance(row, dict) and isinstance(row.get("id"), str)
                and row["id"], f"invalid {label} identity")
        require(row["id"] not in result, f"duplicate {label}: {row['id']}")
        result[row["id"]] = row
    return result


def strings(values, label):
    require(isinstance(values, list) and values
            and all(isinstance(v, str) and v.strip() for v in values)
            and len(values) == len(set(values)), f"invalid {label}")


def validate(document, root=ROOT):
    require(type(document["schema_version"]) is int and document["schema_version"] == 1
            and type(document["inventory_version"]) is int
            and document["inventory_version"] >= 1, "invalid inventory version")
    require(re.fullmatch(r"[0-9a-f]{40}", document["reviewed_source_commit"]),
            "missing exact reviewed source commit")
    require(document["scope_note"].strip(), "missing scope/qualification")
    areas = keyed(document["requirement_areas"], "area")
    require(set(areas) == {f"LC-{i:03d}" for i in range(1, 31)},
            "all 30 mandatory areas must remain")
    for area in areas.values():
        require(area["minimum_gate"] in {"LB0", "LB1"}, "invalid library gate")
        for field in ("required_operations", "required_surfaces", "requirement_ids",
                      "delivery_task_ids"):
            strings(area[field], f"{area['id']}/{field}")
        require(set(area["required_surfaces"]) <= SURFACES, "unknown surface")
        require(all(re.fullmatch(r"CVX-[0-9]{3}", t)
                    for t in area["delivery_task_ids"]), "invalid pending task")
        require(area["family"].strip(), "missing area family")
    for kind, fields in (("conventions", ("currency", "version", "document_status",
                                          "calendar_policy", "index_policy", "terms_policy")),
                         ("models", ("version", "numerical_policy"))):
        require(isinstance(document[kind], dict) and document[kind], f"empty {kind}")
        for row in document[kind].values():
            require(all(isinstance(row.get(k), str) and row[k].strip() for k in fields),
                    f"incomplete {kind} policy")
    observations = document["observations"]
    require(isinstance(observations, dict) and observations, "no observations")
    for identity, row in observations.items():
        require(row["assessment"] in ASSESSMENTS, f"unknown assessment: {identity}")
        require(row["note"].strip() and row["test_execution"].strip(),
                f"unqualified source observation: {identity}")
        strings(row["gaps"], f"{identity}/gaps")
        require(isinstance(row["public_apis"], list), "invalid API observations")
    combinations = keyed(document["combinations"], "combination")
    covered = set()
    covered_surfaces = {key: set() for key in areas}
    expected_records = {}
    for identity, row in combinations.items():
        require(row["area_id"] in areas, f"unknown area: {identity}")
        area = areas[row["area_id"]]
        require(row["convention_id"] in document["conventions"]
                and row["model_id"] in document["models"], f"unknown dimensions: {identity}")
        require(identity == "/".join((row["area_id"], row["product"],
                                     row["convention_id"], row["model_id"])),
                "combination identity must name its dimensions")
        require(row["mandatory"] is True and row["economic_terms"].strip(),
                f"mandatory scope/economics missing: {identity}")
        require(row["scope"] in {"initial_certificate", "retained_compatibility"},
                "unknown scope")
        strings(row["surfaces"], f"{identity}/surfaces")
        require(set(row["surfaces"]) <= set(area["required_surfaces"]),
                "unexpected combination surface")
        if row["area_id"] != "LC-029":
            require(row["surfaces"] == area["required_surfaces"],
                    f"required public surface omitted: {identity}")
        strings(row["build_profiles"], f"{identity}/build profiles")
        require(row["build_profiles"] == (PROFILES if "rust" in row["surfaces"]
                                         else ["client-host-pending"]),
                "build compatibility requirement omitted")
        strings(row["observation_ids"], f"{identity}/observations")
        require(set(row["observation_ids"]) <= set(observations), "unknown evidence observation")
        covered.add(row["area_id"])
        covered_surfaces[row["area_id"]].update(row["surfaces"])
        for operation in area["required_operations"]:
            for surface in row["surfaces"]:
                key = f"{identity}/{operation}/{surface}"
                expected_records[key] = (identity, operation, surface, area)
    require(covered == set(areas), "mandatory area has no product/curve combination")
    require(all(covered_surfaces[key] == set(row["required_surfaces"]) for key, row in areas.items()),
            "mandatory advertised client surface omitted")
    records = keyed(document["records"], "record")
    require(set(records) == set(expected_records), "missing/unexpected mandatory operation or surface record")
    for identity, row in records.items():
        combo, operation, surface, area = expected_records[identity]
        require((row["combination_id"], row["operation"], row["surface"])
                == (combo, operation, surface), "record dimensions disagree")
        require(row["mandatory"] is True, "required record cannot become optional")
        require(row["status"] in {"Planned", "ImplementedUnverified"},
                "mandatory record cannot be NotApplicable/Deferred or Verified by this inventory")
        strings(row["pending_task_ids"], f"{identity}/pending tasks")
        require(set(row["pending_task_ids"]) <= set(area["delivery_task_ids"]),
                "record needs a pending owning implementation task")
        if row["status"] == "ImplementedUnverified":
            evidence = [observations[o] for o in combinations[combo]["observation_ids"]]
            require(any(o["assessment"] == "candidate" and o["public_apis"]
                        and any(a["kind"] == "test" for a in o["anchors"]) for o in evidence)
                    and all(o["assessment"] == "candidate" for o in evidence),
                    "declaration-only, partial, approximate or missing dispatch cannot count as implemented")
    excluded = keyed(document["outside_or_inapplicable"], "excluded/inapplicable")
    for row in excluded.values():
        require(row["mandatory"] is False
                and row["status"] in {"NotApplicable", "Deferred"}
                and isinstance(row.get("reason"), str) and row["reason"].strip(),
                "inapplicable/deferred needs a reason and cannot waive mandatory coverage")
        require(row["product"].strip() and row["operation"].strip(), "unscoped exclusion")
    sources = document["inspected_sources"]
    require(isinstance(sources, list) and sources, "no inspected source evidence")
    require(len(sources) == len(set(sources)), "duplicate inspected source")
    contents = {path: file_path(root, path).read_bytes().decode("utf-8") for path in sources}
    for identity, row in observations.items():
        for item in row["anchors"]:
            require(item["path"] in contents and item["text"]
                    and item["text"] in contents[item["path"]],
                    f"missing inspected anchor: {identity}/{item}")
            require(item["kind"] in {"code", "test"}, "unknown anchor kind")
            if item["kind"] == "test":
                require(re.search(r"\b(?:fn|class)\s", item["text"]),
                        "test anchor must identify a test function/class")
    return document


def preserve(old, new):
    """Version/hash refresh must not drop or weaken admitted requirements."""
    for section in ("requirement_areas", "combinations", "records", "outside_or_inapplicable"):
        previous = keyed(old[section], section)
        current = keyed(new[section], section)
        require(set(previous) <= set(current), f"coverage removed from {section}")
        for key, row in previous.items():
            candidate = current[key]
            if section == "requirement_areas":
                require(row["minimum_gate"] == candidate["minimum_gate"], "library gate weakened")
                for field in ("required_operations", "required_surfaces", "requirement_ids", "delivery_task_ids"):
                    require(set(row[field]) <= set(candidate[field]), f"required {field} removed")
            elif section == "combinations":
                for field in ("area_id", "product", "convention_id", "model_id", "scope",
                              "mandatory", "economic_terms"):
                    require(row[field] == candidate[field], f"economic/scope dimension changed: {key}/{field}")
                for field in ("surfaces", "build_profiles"):
                    require(set(row[field]) <= set(candidate[field]), f"required {field} removed")
            elif section == "records":
                for field in ("combination_id", "operation", "surface", "mandatory"):
                    require(row[field] == candidate[field], f"required record weakened: {key}")
                require(set(row["pending_task_ids"]) <= set(candidate["pending_task_ids"]),
                        f"owning implementation task removed: {key}")
            else:
                require(row == candidate, f"exclusion policy changed: {key}; explicit scope review required")
    for section in ("conventions", "models"):
        require(all(new[section].get(k) == v for k, v in old[section].items()),
                f"{section} changed; add a new reviewed profile instead of reinterpreting existing records")
    if old != new:
        require(new["inventory_version"] > old["inventory_version"],
                "changed inventory must increment its version")


def load(root=ROOT, base=None):
    document = validate(decode(file_path(root, MANIFEST).read_bytes()), root)
    if base:
        revision = subprocess.check_output(["git", "rev-parse", "--verify", f"{base}^{{commit}}"],
                                           cwd=root, text=True).strip()
        previous = subprocess.run(["git", "show", f"{revision}:{MANIFEST}"], cwd=root, capture_output=True)
        if previous.returncode == 0:
            preserve(decode(previous.stdout), document)
        else:
            # Initial inventory admission only: distinguish missing path from git failure.
            tree = subprocess.check_output(["git", "ls-tree", revision, "--", MANIFEST], cwd=root)
            require(not tree, "cannot read existing inventory at base")
    return document


def report(document, gate=None):
    areas = keyed(document["requirement_areas"], "area")
    combos = keyed(document["combinations"], "combination")
    records = [r for r in document["records"] if gate != "LB0"
               or areas[combos[r["combination_id"]]["area_id"]]["minimum_gate"] == "LB0"]
    counts = Counter(r["status"] for r in records)
    area_results = []
    for identity, area in areas.items():
        selected = [r for r in records if combos[r["combination_id"]]["area_id"] == identity]
        if not selected:
            continue
        evidence = sorted({o for r in selected for o in combos[r["combination_id"]]["observation_ids"]})
        area_results.append({"id": identity, "family": area["family"], "minimum_gate": area["minimum_gate"],
                             "records": len(selected), "pending_tasks": area["delivery_task_ids"],
                             "observations": evidence, "gaps": sorted({g for o in evidence
                                 for g in document["observations"][o]["gaps"]})})
    return {"status": "inventory_valid" if gate is None else "incomplete",
            "inventory_version": document["inventory_version"], "inventory_only": True,
            "certified": False, "requested_gate": gate, "mandatory_records": len(records),
            "record_status_counts": dict(counts), "blocked_records": len(records),
            "source_files": len(document["inspected_sources"]), "combinations": len(combos),
            "selected_combinations": len({r["combination_id"] for r in records}),
            "reviewed_source_commit": document["reviewed_source_commit"],
            "records_by_surface": dict(Counter(r["surface"] for r in records)),
            "record_evidence_policy": "Pending task mapping is exhaustive. Family source/test observations are reuse candidates; they do not prove each operation/convention/surface combination.",
            "areas": area_results, "source_observations_are_execution_evidence": False,
            "outside_or_inapplicable": document["outside_or_inapplicable"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--base", help="Preserve all requirements admitted at this git commit")
    parser.add_argument("--gate", choices=("LB0", "LB1"), help="Fail explicitly while certification is incomplete")
    parser.add_argument("--report", type=Path, help="Save full coverage/gap report")
    args = parser.parse_args()
    try:
        result = report(load(args.root.resolve(), args.base), args.gate)
        result["inventory_file_sha256"] = hashlib.sha256(
            file_path(args.root.resolve(), MANIFEST).read_bytes()).hexdigest()
        if args.report:
            args.report.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({k: v for k, v in result.items() if k not in ("areas", "outside_or_inapplicable")}, sort_keys=True))
        return 1 if args.gate else 0
    except (ValueError, KeyError, TypeError, OSError, subprocess.CalledProcessError) as error:
        print(f"coverage rejected: {error}", file=sys.stderr)
        # A rejected manifest must not leave an old successful report behind.
        if args.report:
            try:
                args.report.write_text(json.dumps({"status": "rejected", "certified": False,
                                                  "error": str(error)}, indent=2) + "\n")
            except OSError as write_error:
                print(f"cannot replace report: {write_error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
