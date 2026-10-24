#!/usr/bin/env python3
"""Deterministic supplied helper requests; native outputs are not price oracles."""
import argparse
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATH = ROOT / "tests/fixtures/library/rate-helpers.json"


def corpus():
    original = json.loads((ROOT / "tests/fixtures/library/cases.json").read_bytes())
    base = next(c["request"] for c in original["cases"] if c["id"] == "calibration.mixed_rates")
    graph = base["operation"]["input"]
    reference = json.loads((ROOT / "tests/fixtures/curves/mixed-rates.json").read_bytes())
    zeros = reference["cases"][0]["expected_zeros"]
    curves = []
    for rate in graph["rates"]:
        recipe = rate["recipe"]
        curve = copy.deepcopy(recipe["curve"])
        curve["dependencies"] = []
        curve["calibration"] = "Direct"
        dates = [curve["reference_date"]] + recipe["pillars"]
        values = [zeros[curve["key"]["id"]][0]] + zeros[curve["key"]["id"]]
        curve["source"] = {"Nodes": [{"pillar": {"Date": d}, "quote": {
            "value": float(v), "unit": "DecimalRate"}} for d, v in zip(dates, values)]}
        curves.append(curve)
    alias = copy.deepcopy(graph["rates"][0]["recipe"]["curve"])
    alias.update(key={"id": "P.SOFR", "version": "1"},
                 role={"kind": "Projection", "index_id": "USD.SOFR"},
                 dependencies=[{"id": "D.OIS", "version": "1"}], calibration="Direct",
                 source={"OvernightProjection": {"discount": {"id": "D.OIS", "version": "1"},
                                                  "index_id": "USD.SOFR"}})
    curves.append(alias)
    inputs = copy.deepcopy(base["inputs"])
    inputs.update(curves=curves, calibration=None)
    inputs["publications"] = [p for p in inputs["publications"]
                              if "2026-09-16" <= p["observation_date"] <= "2027-10-11"]
    requests = {}
    for name, ri, hi in (("deposit", 1, 0), ("fra", 1, 1), ("irs", 1, 2),
                         ("ois", 0, 2), ("rate_future", 0, 1), ("basis", 2, 2)):
        rate = copy.deepcopy(graph["rates"][ri])
        recipe = rate["recipe"]
        helper = recipe["helpers"][hi]
        recipe["helpers"] = [helper]
        recipe["pillars"] = [helper["pillar"]]
        recipe["curve"]["source"] = {"Helpers": [helper["id"]]}
        if name in ("deposit", "fra"):
            recipe["curve"]["dependencies"] = []
        if name in ("deposit", "fra", "irs", "basis"):
            recipe["bootstrap"] = None
            recipe["curve"]["calibration"] = {"Global": {
                "initial_zeros": [0.02], "quote_scales": [0.01], "weights": [1.0],
                "lower_zero": -0.08, "upper_zero": 0.15,
                "objective_tolerance": 1e-12, "max_iterations": 100}}
        rate["quote_versions"] = [41 + hi]
        rate["futures_calendar_stamps"] = [rate["futures_calendar_stamps"][hi]]
        supplied = copy.deepcopy(inputs)
        indices = set()
        def collect_indices(value):
            if isinstance(value, dict):
                for key, child in value.items():
                    if key in ("index_id", "index") and isinstance(child, str):
                        indices.add(child)
                    collect_indices(child)
            elif isinstance(value, list):
                for child in value:
                    collect_indices(child)
        collect_indices(helper["terms"])
        supplied["publications"] = [p for p in supplied["publications"]
            if p["index_id"] in indices and p["observation_date"] <= helper["pillar"]]
        requests[name] = dict(schema_version=1, inputs=supplied, operation={
            "kind": "helper", "input": {"kind": "rate_reprice", "input": rate,
                                        "context": copy.deepcopy(graph["rate_context"])}})
    cases = []

    def add(name, suffix, mutation=None, error=None):
        request = copy.deepcopy(requests[name])
        if mutation:
            mutation(request)
        cases.append(dict(id=f"rate_helpers.{name}.{suffix}", request=request, expected_error=error))

    def recipe(request):
        return request["operation"]["input"]["input"]["recipe"]

    def helper(request):
        return recipe(request)["helpers"][0]

    def curve(request):
        key = recipe(request)["curve"]["key"]
        return next(c for c in request["inputs"]["curves"] if c["key"] == key)

    def off_market(request):
        h = helper(request)
        # Deposit/FRA/futures quote is an observation. No quoted profile is lost.
        h["quote"]["value"] += 1

    for name in requests:
        add(name, "positive")
        add(name, "missing_curve", lambda r: r["inputs"].update(curves=[]), "missing_input")
        add(name, "wrong_version", lambda r: recipe(r)["curve"]["key"].update(version="2"), "missing_input")
        add(name, "wrong_role", lambda r: curve(r).update(role={"kind": "Government", "benchmark_id": "TEST"}), "invalid_input")
        add(name, "missing_calendar", lambda r: r["inputs"].update(calendars=[]), "missing_input")
        add(name, "zero_tolerance", lambda r: helper(r).update(tolerance=0.0), "invalid_input")
    add("deposit", "negative_rate", lambda r: [n["quote"].update(value=-0.003) for n in curve(r)["source"]["Nodes"]])
    add("deposit", "decimal_unit", lambda r: helper(r)["quote"].update(unit="DecimalRate", value=0.036535725781932684))
    add("deposit", "off_market", off_market)
    add("deposit", "zero_length", lambda r: helper(r)["terms"]["terms"].update(maturity_date="2026-10-07"), "invalid_input")
    add("deposit", "wrong_unit", lambda r: helper(r)["quote"].update(unit="FuturePrice100MinusRate"), "invalid_input")
    add("deposit", "missing_quote_revision", lambda r: r["operation"]["input"]["input"].update(quote_versions=[]), "invalid_input")
    def repeated_deposits(request, count):
        rate = request["operation"]["input"]["input"]
        rec = rate["recipe"]
        first = copy.deepcopy(rec["helpers"][0])
        rec["helpers"] = []
        for i in range(count):
            h = copy.deepcopy(first)
            h["id"] = f"explicit-deposit-{i:03d}"
            rec["helpers"].append(h)
        rec["curve"]["source"] = {"Helpers": [h["id"] for h in rec["helpers"]]}
        rec["bootstrap"] = None
        rec["overlap_policy"] = "RetainAllGlobal"
        rec["curve"]["calibration"] = {"Global": {
            "initial_zeros": [0.02], "quote_scales": [0.01] * count, "weights": [1.0] * count,
            "lower_zero": -0.08, "upper_zero": 0.15,
            "objective_tolerance": 1e-12, "max_iterations": 100}}
        rate["quote_versions"] = list(range(1001, 1001 + count))
        rate["futures_calendar_stamps"] = [None] * count
    add("deposit", "max_helpers", lambda r: repeated_deposits(r, 128))
    add("deposit", "helper_limit", lambda r: repeated_deposits(r, 129), "resource_limit")

    def known_fra(request):
        t = helper(request)["terms"]["terms"]
        t["fixing_date"] = "2026-10-06"
        request["inputs"]["publications"] = [p for p in request["inputs"]["publications"]
            if (p["index_id"], p["observation_date"]) != (t["index_id"], t["fixing_date"])]
        request["inputs"]["publications"].append(dict(index_id=t["index_id"], observation_date=t["fixing_date"],
                                                        expected_publication="2026-10-07T08:00:00Z"))
        request["inputs"]["fixings"].append(dict(index_id=t["index_id"], observation_date=t["fixing_date"],
            published_at="2026-10-07T08:00:00Z", known_at="2026-10-07T08:01:00Z", revision=3,
            rate="0.042125", source_id="declared-CVX063-helper-control", status="Final"))

    add("fra", "known", known_fra)
    add("fra", "off_market", off_market)
    add("fra", "at_end", lambda r: helper(r)["terms"]["terms"].update(settlement="AtEnd", settlement_date="2026-12-16"))
    def missing_known(request):
        known_fra(request)
        request["inputs"]["fixings"].pop()
    add("fra", "missing_history", missing_known, "missing_input")
    add("fra", "missing_publication", lambda r: r["inputs"].update(publications=[]), "invalid_input")
    add("fra", "wrong_settlement", lambda r: helper(r)["terms"]["terms"].update(settlement_date="2026-10-31"), "invalid_input")
    add("irs", "receiver", lambda r: helper(r)["terms"].update(side="ReceiveFixed"))
    add("irs", "missing_history", lambda r: r["inputs"].update(fixings=[]), "missing_input")
    add("irs", "mismatched_profile", lambda r: helper(r)["terms"]["fixed"]["rates"][0].update(value="0.9"), "unsupported")
    add("ois", "negative_curve", lambda r: [n["quote"].update(value=-0.003) for n in curve(r)["source"]["Nodes"]])
    add("ois", "missing_history", lambda r: r["inputs"].update(fixings=[]), "missing_input")
    add("ois", "fake_zero_basis_view", lambda r: next(c for c in r["inputs"]["curves"] if c["key"]["id"] == "P.SOFR").update(
        dependencies=[], source=copy.deepcopy(r["inputs"]["curves"][0]["source"])), "invalid_input")
    add("rate_future", "arithmetic", lambda r: helper(r)["terms"]["contract"]["settlement"]["accrual"].update(averaging="arithmetic"))
    add("rate_future", "off_market", off_market)
    add("rate_future", "missing_history", lambda r: r["inputs"].update(fixings=[]), "missing_input")
    add("rate_future", "missing_stamp", lambda r: r["operation"]["input"]["input"].update(futures_calendar_stamps=[None]), "missing_input")
    add("rate_future", "bad_projection_stamp", lambda r: helper(r)["terms"]["projection_stamp"].update(id="other"), "invalid_input")
    add("basis", "negative_curve", lambda r: [n["quote"].update(value=-0.003) for n in curve(r)["source"]["Nodes"]])
    add("basis", "missing_history", lambda r: r["inputs"].update(fixings=[]), "missing_input")
    add("basis", "missing_dependency", lambda r: recipe(r)["curve"].update(dependencies=[]), "invalid_input")
    return dict(schema_version=1, certified=False,
                qualification="Supplied full rate-helper diagnostics and native typed failures; original independent mixed-rate reference remains separate.",
                cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    args = parser.parse_args()
    content = (json.dumps(corpus(), separators=(",", ":")) + "\n").encode()
    if args.write:
        PATH.write_bytes(content)
    elif PATH.read_bytes() != content:
        raise ValueError("Supplied helper requests changed; inspect original economics")
    print(json.dumps(dict(status="passed", cases=len(corpus()["cases"]), certified=False)))


if __name__ == "__main__":
    main()
