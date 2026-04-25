"""
Compare Convex vs QuantLib output row-for-row and write a reconciliation report.

Inputs:   reconciliation/convex.csv  and  reconciliation/ql.csv
Output:   reconciliation/reconciliation_report.md

A metric passes if the absolute difference is within its tolerance (see
TOLERANCES below). The exit code is non-zero if any row fails, so the
script is CI-friendly.
"""
from __future__ import annotations

import csv
import pathlib
import sys
from collections import defaultdict
from dataclasses import dataclass

HERE = pathlib.Path(__file__).parent

# Per-metric absolute tolerance. Chosen to detect real numerical divergence
# but absorb normal floating-point noise. Tightened later as the suite matures.
TOLERANCES: dict[str, float] = {
    "clean_price_pct":    1e-6,
    "dirty_price_pct":    1e-6,
    "accrued":            1e-8,
    "ytm_decimal":        1e-7,   # 0.001 bp
    "macaulay_duration":  1e-4,
    "modified_duration":  1e-4,
    "convexity":          1e-3,
    "dv01_per_100":       1e-7,
    # TIPS nominal add-ons (both sides compute as real × CPI ratio).
    "cpi_index_ratio":        1e-10,
    "nominal_clean_price_pct": 1e-6,
    "nominal_dirty_price_pct": 1e-6,
    "nominal_accrued":         1e-8,
}


@dataclass
class Row:
    bond_id: str
    currency: str
    metric: str
    value: float
    reference_yield: float
    curve_used: str


def load(path: pathlib.Path) -> dict[tuple[str, str], Row]:
    out: dict[tuple[str, str], Row] = {}
    with path.open() as fh:
        for r in csv.DictReader(fh):
            key = (r["bond_id"], r["metric"])
            out[key] = Row(
                bond_id=r["bond_id"],
                currency=r["currency"],
                metric=r["metric"],
                value=float(r["value"]),
                reference_yield=float(r["reference_yield"]),
                curve_used=r["curve_used"],
            )
    return out


def format_delta(delta: float, tol: float) -> str:
    if abs(delta) < 1e-12:
        return "0"
    return f"{delta:+.3e}"


def main() -> int:
    convex = load(HERE / "convex.csv")
    ql = load(HERE / "ql.csv")

    keys = sorted(set(convex) | set(ql))
    per_bond: dict[str, list[tuple[str, float, float, float, bool, str]]] = defaultdict(list)
    pass_count = 0
    fail_count = 0
    missing = 0

    for key in keys:
        bond_id, metric = key
        c = convex.get(key)
        q = ql.get(key)
        if c is None or q is None:
            missing += 1
            continue
        tol = TOLERANCES.get(metric, 1e-6)
        delta = q.value - c.value
        ok = abs(delta) <= tol
        per_bond[bond_id].append((metric, c.value, q.value, delta, ok, c.curve_used))
        if ok:
            pass_count += 1
        else:
            fail_count += 1

    # --------------------------------------------------- render markdown

    out = HERE / "reconciliation_report.md"
    lines: list[str] = []
    lines.append("# Convex ↔ QuantLib reconciliation report\n")
    lines.append(f"Valuation date: 2025-12-31  ")
    lines.append(f"Convex rows: {len(convex)}  ")
    lines.append(f"QuantLib rows: {len(ql)}  ")
    lines.append(f"**Passes: {pass_count}**  ")
    lines.append(f"**Fails: {fail_count}**  ")
    if missing:
        lines.append(f"Missing on one side: {missing}  ")
    lines.append("")

    # Per-bond summary table
    lines.append("## Per-bond summary\n")
    lines.append("| Bond | Curve | Passes | Fails |")
    lines.append("|---|---|---|---|")
    for bond_id, rows in sorted(per_bond.items()):
        curve = rows[0][5]
        ok = sum(1 for r in rows if r[4])
        bad = sum(1 for r in rows if not r[4])
        lines.append(f"| `{bond_id}` | {curve} | {ok} | {bad} |")
    lines.append("")

    # Detail section per bond
    lines.append("## Detail per bond\n")
    for bond_id, rows in sorted(per_bond.items()):
        lines.append(f"### `{bond_id}`\n")
        lines.append("| Metric | Convex | QuantLib | Δ (QL − CX) | Tol | Pass |")
        lines.append("|---|---:|---:|---:|---:|:---:|")
        for metric, cx, qx, delta, ok, _curve in rows:
            tol = TOLERANCES.get(metric, 1e-6)
            mark = "✓" if ok else "✗"
            lines.append(
                f"| {metric} | {cx:.10f} | {qx:.10f} | {format_delta(delta, tol)} | "
                f"{tol:.0e} | {mark} |"
            )
        lines.append("")

    # Fails upfront for quick triage
    if fail_count:
        lines.append("## Fails (for triage)\n")
        lines.append("| Bond | Metric | Convex | QuantLib | Δ | Tol |")
        lines.append("|---|---|---:|---:|---:|---:|")
        for bond_id, rows in sorted(per_bond.items()):
            for metric, cx, qx, delta, ok, _curve in rows:
                if not ok:
                    tol = TOLERANCES.get(metric, 1e-6)
                    lines.append(
                        f"| `{bond_id}` | {metric} | {cx:.10f} | {qx:.10f} | "
                        f"{format_delta(delta, tol)} | {tol:.0e} |"
                    )
        lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"reconcile: wrote {out}", file=sys.stderr)
    print(f"reconcile: {pass_count} pass / {fail_count} fail", file=sys.stderr)
    return 1 if fail_count else 0


if __name__ == "__main__":
    sys.exit(main())
