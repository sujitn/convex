#!/usr/bin/env python3
"""Supplemental supplied requests; this generator creates no price expectations."""
import argparse
import copy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "tests/fixtures/library/operations.json"


def corpus():
    original = {c["id"]: c["request"] for c in json.loads(
        (ROOT / "tests/fixtures/library/cases.json").read_bytes())["cases"]}
    cases = []
    empty = dict(calendars=[], calibration=None, curves=[], fixings=[],
                 helper_calendars=[], publications=[])

    def add(identity, operation, inputs=None, error=None):
        request = dict(schema_version=1, inputs=copy.deepcopy(empty if inputs is None else inputs),
                       operation=copy.deepcopy(operation))
        cases.append(dict(id=identity, request=request, expected_error=error))

    base = copy.deepcopy(original["curves.build"]["inputs"]["curves"][0])
    base.update(reference_date="2024-02-28", extrapolation="None")
    base["key"] = dict(id="supplied.operation.discount", version="synthetic-1")
    base["source"]["Nodes"] = [dict(pillar={"Tenor": t}, quote=dict(value=v, unit="DecimalRate"))
                              for t, v in ((0.0, -0.015), (1.0, 0.02), (5.0, 0.04))]
    inputs = copy.deepcopy(empty); inputs["curves"] = [base]

    def time(identity, action, resources=inputs, error=None):
        add(identity, dict(kind="curve_time", curve=resources["curves"][0]["key"], action=action),
            resources, error)

    for basis in ("Act365Fixed", "Act360", "ActActIsda"):
        resources = copy.deepcopy(inputs); resources["curves"][0]["time_basis"] = basis
        time("time.date." + basis, dict(kind="date_to_tenor", date="2024-03-01"), resources)
        time("time.origin." + basis, dict(kind="date_to_tenor", date="2024-02-28"), resources)
        for rounding in ("AtOrBefore", "AtOrAfter", "Nearest"):
            time("time.inverse." + basis + "." + rounding,
                 dict(kind="tenor_to_date", tenor=0.0041, rounding=rounding), resources)
    time("time.inverse.origin", dict(kind="tenor_to_date", tenor=0.0, rounding="Nearest"))
    time("time.before_reference", dict(kind="date_to_tenor", date="2024-02-27"), error="invalid_input")
    time("time.over_span", dict(kind="date_to_tenor", date="2175-03-01"), error="invalid_input")
    time("time.negative", dict(kind="tenor_to_date", tenor=-0.1, rounding="Nearest"), error="invalid_input")
    time("time.over_tenor", dict(kind="tenor_to_date", tenor=150.1, rounding="Nearest"), error="invalid_input")
    end = copy.deepcopy(inputs); end["curves"][0]["reference_date"] = "9999-12-31"
    time("time.last_date_origin", dict(kind="tenor_to_date", tenor=0.0, rounding="Nearest"), end)
    time("time.last_date_no_future", dict(kind="tenor_to_date", tenor=0.1, rounding="Nearest"), end, "invalid_input")
    missing = dict(kind="curve_time", curve=dict(base["key"], version="absent"),
                   action=dict(kind="date_to_tenor", date="2024-03-01"))
    add("time.missing_revision", missing, inputs, "missing_input")

    dated = dict(kind="dated", start="2024-02-28", end="2024-03-01", day_count="ActActIsda")
    def convert(identity, action, period=dated, error=None):
        add(identity, dict(kind="rate_conversion", period=period, action=action), error=error)

    for compounding in ("Simple", "Continuous", "Annual", "SemiAnnual", "Quarterly", "Monthly", "Daily"):
        convert("conversion.discount." + compounding,
                dict(kind="discount_from_rate", rate=-0.01, compounding=compounding))
        convert("conversion.rate." + compounding,
                dict(kind="rate_from_discount", discount=1.0001, compounding=compounding))
        convert("conversion.equivalent." + compounding,
                dict(kind="convert_rate", rate=-0.01, **{"from": compounding}, to="Simple"))
    for basis in ("Act360", "Act365Fixed", "Thirty360US", "Thirty360E"):
        convert("conversion.dated." + basis, dict(kind="discount_from_rate", rate=0.02, compounding="Annual"),
                dict(dated, day_count=basis))
    discount = dict(kind="discount_from_rate", rate=0.02, compounding="Simple")
    convert("conversion.zero_origin", discount, dict(kind="year_fraction", value=0.0))
    convert("conversion.maximum_span", discount, dict(kind="year_fraction", value=152.5))
    convert("conversion.short_period", dict(kind="convert_rate", rate=1e-9, **{"from": "Continuous"}, to="SemiAnnual"),
            dict(kind="year_fraction", value=1e-10))
    convert("conversion.rate.zero", dict(kind="rate_from_discount", discount=1.0, compounding="Continuous"))
    for identity, period, action, error in (
        ("negative_time", dict(kind="year_fraction", value=-0.01), discount, "invalid_input"),
        ("over_span", dict(kind="year_fraction", value=152.51), discount, "invalid_input"),
        ("equal_dates", dict(dated, end="2024-02-28"), discount, "invalid_input"),
        ("reversed_dates", dict(dated, end="2024-02-27"), discount, "invalid_input"),
        ("missing_icma", dict(dated, day_count="ActActIcma"), discount, "unsupported"),
        ("simple_base", dict(kind="year_fraction", value=2.0), dict(discount, rate=-0.5), "no_convergence"),
        ("periodic_base", dated, dict(discount, rate=-1.0, compounding="Annual"), "no_convergence"),
        ("nonpositive_discount", dated, dict(kind="rate_from_discount", discount=0.0, compounding="Continuous"), "no_convergence"),
        ("zero_inverse", dict(kind="year_fraction", value=0.0), dict(kind="rate_from_discount", discount=1.0, compounding="Annual"), "no_convergence"),
        ("zero_equivalent", dict(kind="year_fraction", value=0.0), dict(kind="convert_rate", rate=0.02, **{"from": "Annual"}, to="Annual"), "no_convergence"),
    ):
        convert("conversion." + identity, action, period, error)

    def query(identity, resources, curve, query, error=None):
        add(identity, dict(kind="curve_query", curve=curve, query=query), resources, error)

    for representation in ("DiscountFactors", {"ZeroRates": {"compounding": "SemiAnnual"}}):
        name = "df" if isinstance(representation, str) else "zero"
        view = copy.deepcopy(base)
        view.update(key=dict(id="supplied.operation.view." + name, version="synthetic-1"),
                    representation=representation, source={"DiscountEquivalent": {"base": base["key"]}},
                    dependencies=[base["key"]])
        resources = copy.deepcopy(inputs); resources["curves"].append(view)
        query("view.equivalent." + name, resources, view["key"], dict(kind="DiscountFactor", point={"Tenor": 0.3}))
        query("view.equivalent." + name + ".origin", resources, view["key"], dict(kind="DiscountFactor", point={"Tenor": 0.0}))
        query("view.equivalent." + name + ".out_of_domain", resources, view["key"], dict(kind="DiscountFactor", point={"Tenor": 6.0}), "invalid_input")
    for policy in ("StickyForward", "StickyZero"):
        view = copy.deepcopy(base)
        view.update(key=dict(id="supplied.operation.roll." + policy, version="synthetic-1"),
                    reference_date="2024-08-28", representation="DiscountFactors",
                    source={"Rolled": {"base": base["key"], "policy": policy}}, dependencies=[base["key"]])
        resources = copy.deepcopy(inputs); resources["curves"].append(view)
        query("roll." + policy, resources, view["key"], dict(kind="DiscountFactor", point={"Date": "2025-02-28"}))
        query("roll." + policy + ".origin", resources, view["key"], dict(kind="DiscountFactor", point={"Tenor": 0.0}))
        bad = copy.deepcopy(resources); bad["curves"][-1]["reference_date"] = "2024-02-27"
        query("roll." + policy + ".backward", bad, view["key"], dict(kind="DiscountFactor", point={"Tenor": 0.1}), "invalid_input")
        bad = copy.deepcopy(resources); bad["curves"][-1]["reference_date"] = "2030-01-01"
        query("roll." + policy + ".beyond_original_domain", bad, view["key"], dict(kind="DiscountFactor", point={"Tenor": 0.1}), "invalid_input")
    spread = copy.deepcopy(base)
    spread.update(key=dict(id="supplied.operation.spread", version="synthetic-1"), dependencies=[base["key"]],
                  source={"ContinuousZeroSpread": {"base": base["key"], "spread": dict(value=-25.0, unit="BasisPoints")}})
    resources = copy.deepcopy(inputs); resources["curves"].append(spread)
    query("view.spread", resources, spread["key"], dict(kind="DiscountFactor", point={"Date": "2025-02-28"}))
    query("view.spread.origin", resources, spread["key"], dict(kind="DiscountFactor", point={"Tenor": 0.0}))
    bad = copy.deepcopy(resources); bad["curves"][-1]["dependencies"][0]["version"] = "absent"
    query("view.missing_dependency", bad, spread["key"], dict(kind="DiscountFactor", point={"Tenor": 0.1}), "missing_input")
    bad = copy.deepcopy(resources); bad["curves"][-1]["interpolation"] = "LogLinear"
    query("view.changed_interpolation", bad, spread["key"], dict(kind="DiscountFactor", point={"Tenor": 0.1}), "invalid_input")
    query("query.negative_rate", inputs, base["key"], dict(kind="ZeroRate", point={"Tenor": 0.1}, compounding="Annual", day_count="Act365Fixed"))
    query("query.dated_forward", inputs, base["key"], dict(kind="ForwardRate", start={"Date": "2024-03-01"}, end={"Date": "2024-08-28"}, compounding="Simple", day_count="Act360"))
    query("query.forward_origin", inputs, base["key"], dict(kind="ForwardRate", start={"Date": "2024-02-28"}, end={"Date": "2024-02-29"}, compounding="Simple", day_count="Act360"))
    query("query.forward_same_date", inputs, base["key"], dict(kind="ForwardRate", start={"Date": "2024-02-28"}, end={"Date": "2024-02-28"}, compounding="Simple", day_count="Act360"), "invalid_input")
    query("query.zero_at_origin", inputs, base["key"], dict(kind="ZeroRate", point={"Tenor": 0.0}, compounding="Annual", day_count="Act365Fixed"), "invalid_input")

    controls = original["curve.fit.dated"]["operation"]["controls"]
    graph = dict(rates=[], credit=[], cash=[], rate_context=None, controls=controls)
    position = dict(original["bond.quote_risk"]["operation"]["action"]["position"], quantity="5000")
    settings = original["bond.quote_risk"]["operation"]["action"]["settings"]
    snapshot = original["bond.quote_risk"]["operation"]["action"]["snapshot_id"]
    def key(curve, coordinate, quote_type="zero_rate", native="DecimalRate", tenor="parallel"):
        family = "hazard" if quote_type == "hazard_rate" else "credit_spread" if quote_type == "cds_spread" else "interest_rate"
        basis = "model_parameter" if quote_type == "hazard_rate" else "market_quote_recalibrated" if quote_type == "cds_spread" else "zero_node"
        return dict(factor=dict(basis=basis, convention_version=1, currency="USD", family=family,
                        model_version="synthetic.CVX063.operation.v1", quote_type=quote_type,
                        target=dict(kind="curve", curve_id=curve["id"]), tenor=tenor),
                    coordinate=dict(coordinate, curve=curve), native_unit=native,
                    response="rebuild_dependents" if quote_type == "cds_spread" else "frozen_curves",
                    unit="currency_per_annual_rate" if quote_type == "hazard_rate" else "currency_per_bp")

    def risk(identity, resources, graph_input, product, buckets, parallel, quantity=position, error=None):
        data = dict(product=product, buckets=buckets, parallel=parallel, position=quantity,
                    snapshot_id=snapshot, settings=settings, tolerance=dict(absolute=0.1, relative=1e-4))
        operation = dict(kind="calibration", input=graph_input, action=dict(kind="reconcile_buckets", input=data))
        add(identity, operation, resources, error)
        return cases[-1]

    bond = original["bond.spread"]["operation"]
    product = dict(kind="bond", bond=bond["bond"], context=bond["context"],
                   projection=bond["projection"], method=bond["action"]["method"])
    for representation in ("zero", "df"):
        resources = copy.deepcopy(original["bond.spread"]["inputs"])
        recipe = resources["curves"][0]; curve = recipe["key"]
        pillars = (0.0, 1.0, 5.0, 100.0)
        if representation == "df":
            recipe.update(representation="DiscountFactors", interpolation="LogLinear")
            values = (1.0, 0.97, 0.85, 0.05); unit = "DiscountFactor"
        else:
            values = (0.035, 0.035, 0.035, 0.035); unit = "DecimalRate"
        recipe["source"] = {"Nodes": [dict(pillar={"Tenor": t}, quote=dict(unit=unit, value=v)) for t, v in zip(pillars, values)]}
        indices = range(1 if representation == "df" else 0, 4)
        buckets = [key(curve, dict(kind="zero", node=i), tenor="spot") for i in indices]
        parallel = key(curve, dict(kind="zero", node=None))
        case = risk("risk.nodes." + representation, resources, graph, product, buckets, parallel)
        for name, quantity in (("short", "-10000"), ("zero", "0")):
            risk("risk.nodes." + representation + "." + name, resources, graph, product, buckets, parallel,
                 dict(position, quantity=quantity))
        bad = copy.deepcopy(case)
        bad.update(id="risk.nodes." + representation + ".missing_zero_risk", expected_error="invalid_input")
        bad["request"]["operation"]["action"]["input"]["buckets"].pop(); cases.append(bad)
    node = next(c for c in cases if c["id"] == "risk.nodes.zero")
    for name, mutate in (
        ("duplicate", lambda i: i["buckets"].append(copy.deepcopy(i["buckets"][0]))),
        ("empty", lambda i: i.update(buckets=[])),
        ("wrong_currency", lambda i: i["position"].update(currency="EUR")),
        ("wrong_reference", lambda i: i["position"].update(reference_quantity="100")),
        ("invalid_parallel", lambda i: i.update(parallel=copy.deepcopy(i["buckets"][0]))),
        ("invalid_tolerance", lambda i: i.update(tolerance=dict(absolute=0.0, relative=0.0))),
        ("invalid_bump", lambda i: i["settings"].update(bump=0.0)),
        ("incompatible_model", lambda i: i["buckets"][0]["factor"].update(model_version="different.model")),
    ):
        bad = copy.deepcopy(node); bad.update(id="risk.nodes." + name, expected_error="invalid_input")
        mutate(bad["request"]["operation"]["action"]["input"]); cases.append(bad)
    tight = copy.deepcopy(node); tight["id"] = "risk.nodes.nonadditive_diagnostic"
    tight["request"]["operation"]["action"]["input"]["tolerance"] = dict(absolute=1e-20, relative=0.0)
    cases.append(tight)

    cds = copy.deepcopy(original["cds.price"])
    resources = cds["inputs"]; graph_input = resources["calibration"]; resources["calibration"] = None
    product = dict(kind="cds", terms=cds["operation"]["terms"], context=cds["operation"]["context"])
    quantity = dict(currency="USD", quantity=product["terms"]["notional"], quantity_unit="notional", reference_quantity=product["terms"]["notional"])
    recipe = graph_input["credit"][0]; curve = recipe["curve"]["key"]
    quotes = sorted([h["id"], h["quote_version"], h["quote"]["unit"]] for h in recipe["helpers"])
    buckets = [key(curve, dict(kind="quote", helper_id=id, quote_version=v), "cds_spread", u, "spot")
               for id, v, u in quotes]
    parallel = key(curve, dict(kind="parallel_quotes", quotes=quotes), "cds_spread")
    case = risk("risk.quotes.cds", resources, graph_input, product, buckets, parallel, quantity)
    risk("risk.quotes.cds.zero", resources, graph_input, product, buckets, parallel, dict(quantity, quantity="0"))
    for name, mutate in (
        ("missing", lambda i: i["buckets"].pop()),
        ("revision", lambda i: i["buckets"][0]["coordinate"].update(quote_version=999)),
        ("native_unit", lambda i: i["buckets"][0].update(native_unit="PercentRate")),
    ):
        bad = copy.deepcopy(case); bad.update(id="risk.quotes." + name, expected_error="invalid_input")
        mutate(bad["request"]["operation"]["action"]["input"]); cases.append(bad)
    buckets = [key(curve, dict(kind="hazard", node=i), "hazard_rate", tenor="spot") for i in range(1, 7)]
    parallel = key(curve, dict(kind="hazard", node=None), "hazard_rate")
    case = risk("risk.nodes.hazard", resources, graph_input, product, buckets, parallel, quantity)
    risk("risk.nodes.hazard.zero", resources, graph_input, product, buckets, parallel, dict(quantity, quantity="0"))
    bad = copy.deepcopy(case); bad.update(id="risk.nodes.hazard.missing", expected_error="invalid_input")
    bad["request"]["operation"]["action"]["input"]["buckets"].pop(); cases.append(bad)

    return dict(schema_version=1, certified=False,
                qualification="Supplied synthetic operation/transport requests; no independent financial expectations or certificate.", cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--emit", action="store_true")
    args = parser.parse_args()
    data = corpus()
    if args.emit:
        OUTPUT.write_text(json.dumps(data, separators=(",", ":"), sort_keys=True) + "\n")
    elif data != json.loads(OUTPUT.read_bytes()):
        raise ValueError("Supplemental supplied requests changed; review before regenerating")
    print(json.dumps(dict(status="passed", cases=len(data["cases"]), certified=False)))


if __name__ == "__main__":
    main()
