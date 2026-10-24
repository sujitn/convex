#!/usr/bin/env python3
"""Declare structural zero-term roundtrip/rejection inputs; no native values or prices."""
import copy
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
TARGET = ROOT / "tests/fixtures/library/zero-terms-reference.json"
DAY_COUNTS = ["Act360", "Act365Fixed", "Act365Leap", "ActActIsda", "ActActIcma",
              "ActActAfb", "Thirty360US", "Thirty360E", "Thirty360EIsda", "Thirty360German"]
COMPOUNDING = ["Annual", "SemiAnnual", "Quarterly", "Monthly", "Continuous"]


def document():
    original = dict(identifiers=dict(cusip="037833100", isin="US0378331005", figi="BBG000B9XRY4",
        sedol="2046251", ticker="EXPLICIT.ZERO", issuer_name="Supplied issuer",
        issue_name="Supplied original issue"), maturity="2027-08-21", issue_date="2020-01-15",
        issue_price="97.1234567890123456789", day_count="Act365Fixed", compounding="SemiAnnual",
        settlement_days=3, calendar="SUPPLIED.CALENDAR@VERSION-7", currency="USD",
        face_value="1000.000001", redemption_value="1032.500007")
    positive = []
    for currency in ["USD", "EUR", "GBP"]:
        for convention in DAY_COUNTS:
            for compounding in COMPOUNDING:
                terms = copy.deepcopy(original)
                terms.update(currency=currency, day_count=convention, compounding=compounding)
                positive.append(dict(id=f"{currency}.{convention}.{compounding}", terms=terms))
    negative = []
    for value in ["", "Unknown", "ActActISDA", "ACT/360", "Act360 ", "ActActIsdaWithReference", "UnknownVendorBasis"]:
        terms = copy.deepcopy(original)
        terms["day_count"] = value
        negative.append(dict(id="unknown_day_count." + str(len(negative)), terms=terms))
    for field in ["coupon", "cap", "floor", "principal_steps", "exercise", "redemption_rule", "reference_periods", "projection_curve", "provider_payload"]:
        terms = copy.deepcopy(original)
        terms[field] = {"supplied": "must not vanish"}
        negative.append(dict(id="ignored_outer." + field, terms=terms))
    for field in ["rating", "seniority", "provider_payload", "issuer_reference_revision"]:
        terms = copy.deepcopy(original)
        terms["identifiers"][field] = "must not vanish"
        negative.append(dict(id="ignored_identifier." + field, terms=terms))
    for field in original:
        if field == "issue_price":
            continue
        terms = copy.deepcopy(original)
        del terms[field]
        negative.append(dict(id="missing_required." + field, terms=terms))
    for field, value in [("day_count", 360), ("compounding", "Simple"), ("calendar", {}),
                         ("currency", "NOT-A-CURRENCY"), ("face_value", "not-a-decimal"),
                         ("redemption_value", "NaN"), ("settlement_days", -1),
                         ("issue_date", "not-a-date"), ("maturity", "2027-02-30")]:
        terms = copy.deepcopy(original)
        terms[field] = value
        negative.append(dict(id="invalid_type." + field, terms=terms))
    null_issue_price = copy.deepcopy(original)
    null_issue_price["issue_price"] = None
    positive.append(dict(id="explicit_null_issue_price", terms=null_issue_price))
    duplicate = json.dumps(original, separators=(",", ":"))
    duplicate = duplicate[:-1] + ',"day_count":"Act360"}'
    return dict(schema_version=1, provenance="Explicit structural inputs declared before decoder edits. No price, yield or native-generated financial reference.",
        positive_cases=positive, negative_cases=negative, duplicate_field_json=duplicate,
        known_day_counts=DAY_COUNTS, compounding_variants=COMPOUNDING,
        units="Lossless exact original metadata and Decimal strings; economic admission remains in checked pricing/terms owners.", certified=False)


def main():
    value = document()
    data = (json.dumps(value, indent=2) + "\n").encode()
    mode = sys.argv[1] if len(sys.argv) == 2 else "check"
    if mode == "write":
        with TARGET.open("xb") as stream:
            stream.write(data)
    elif mode == "check":
        if TARGET.read_bytes() != data:
            raise ValueError("Structural declaration changed")
    else:
        raise ValueError("Use write or check")
    print(json.dumps(dict(status="passed", positive_cases=len(value["positive_cases"]),
                         negative_cases=len(value["negative_cases"]) + 1, certified=False)))


if __name__ == "__main__":
    main()
