"""
Pull the pieces of curves.json / book.json that are too dynamic or too big to
pre-bake: Treasury CMT for the remaining tenors, the SOFR daily fixing series
2024-01-01 to 2025-12-31 (for FRN reconstruction), and the TIPS index ratio on
2025-12-31.

All sources are free-public primary sources: FRED, NY Fed, TreasuryDirect.
No API keys required.

Run:
    python reconciliation/pull_market_data.py

Writes alongside this file:
    sofr_fixings.csv
    ust_cmt_20251231.csv
    tips_index_ratio_20251231.json
"""
from __future__ import annotations

import csv
import json
import pathlib
import sys
import urllib.request
import urllib.error

HERE = pathlib.Path(__file__).parent
VAL_DATE = "2025-12-31"


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "convex-reconciliation/0.1"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return resp.read()
    except urllib.error.HTTPError as e:
        print(f"HTTP {e.code} fetching {url}: {e.reason}", file=sys.stderr)
        raise


def pull_ust_cmt() -> None:
    """Treasury constant-maturity yields on the valuation date from FRED."""
    series = ["DGS1MO", "DGS3MO", "DGS6MO", "DGS1", "DGS2",
              "DGS3", "DGS5", "DGS7", "DGS10", "DGS20", "DGS30"]
    url = (
        "https://fred.stlouisfed.org/graph/fredgraph.csv"
        f"?id={','.join(series)}&cosd={VAL_DATE}&coed={VAL_DATE}"
    )
    out = HERE / "ust_cmt_20251231.csv"
    out.write_bytes(fetch(url))
    print(f"wrote {out} ({out.stat().st_size} bytes)")


def pull_sofr_fixings() -> None:
    """Daily SOFR from NY Fed 2024-01-01 through 2025-12-31."""
    url = (
        "https://markets.newyorkfed.org/api/rates/secured/sofr/search.json"
        "?startDate=2024-01-01&endDate=2025-12-31"
    )
    raw = fetch(url).decode("utf-8")
    data = json.loads(raw)

    # NY Fed wraps rows under "refRates"; older endpoints used a flat array.
    rows = data.get("refRates") if isinstance(data, dict) else data
    if rows is None:
        print("unexpected SOFR payload shape; dumping raw", file=sys.stderr)
        (HERE / "sofr_fixings.raw.json").write_text(raw)
        return

    out = HERE / "sofr_fixings.csv"
    with out.open("w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["effective_date", "rate_pct", "volume_usd_bn"])
        for r in rows:
            w.writerow([
                r.get("effectiveDate"),
                r.get("percentRate"),
                r.get("volumeInBillions"),
            ])
    print(f"wrote {out} ({len(rows)} rows)")


def pull_tips_index_ratio() -> None:
    """TIPS 91282CNS6 index ratio on the valuation date (daily-published)."""
    cusip = "91282CNS6"
    url = (
        "https://www.treasurydirect.gov/TA_WS/securities/search"
        f"?cusip={cusip}&format=json"
    )
    raw = fetch(url).decode("utf-8")

    # Shape varies; persist raw and try best-effort extraction.
    (HERE / "tips_search_raw.json").write_text(raw)
    try:
        data = json.loads(raw)
    except json.JSONDecodeError:
        print("tips payload was not json; raw saved", file=sys.stderr)
        return

    out = HERE / "tips_index_ratio_20251231.json"
    # Daily index-ratio feeds usually live at a different endpoint; this helper
    # records the search result and flags the manual-pull URL.
    out.write_text(json.dumps({
        "cusip": cusip,
        "valuation_date": VAL_DATE,
        "index_ratio": None,
        "raw_search": data,
        "manual_pull_url": (
            "https://www.treasurydirect.gov/auctions/"
            "announcements-data-results/tips-cpi-data/tips-cpi-detail/"
            f"?cusip={cusip}"
        ),
        "note": (
            "The index-ratio time series lives on the TIPS/CPI detail page. "
            "If this script doesn't capture it automatically, open the URL "
            "above, download the CSV for the December 2025 range, and edit "
            "index_ratio manually."
        ),
    }, indent=2))
    print(f"wrote {out}")


def main() -> int:
    failed = []
    for name, fn in [
        ("UST CMT", pull_ust_cmt),
        ("SOFR fixings", pull_sofr_fixings),
        ("TIPS index ratio", pull_tips_index_ratio),
    ]:
        try:
            fn()
        except Exception as e:  # noqa: BLE001
            print(f"{name} pull failed: {e}", file=sys.stderr)
            failed.append(name)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
