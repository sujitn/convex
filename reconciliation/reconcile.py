"""
Compare Convex vs QuantLib output row-for-row and write a reconciliation report.

Inputs:   reconciliation/convex.csv  and  reconciliation/ql.csv
          reconciliation/convex_<label>.csv and ql_<label>.csv (optional snapshots)
Output:   reconciliation/reconciliation_report.md

A metric passes if the absolute difference is within its tolerance (see
TOLERANCES below). The exit code is non-zero if any row fails, so the
script is CI-friendly. Multi-snapshot runs aggregate fail counts across
snapshots; the report renders one section per snapshot.
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
    # Callable OAS (HW1F trinomial). Loose tolerances pending the
    # event-aligned TimeGrid work in NEXT_STEPS.md §5.2.1 — uniform-Δt vs
    # QL's event-aligned grid leaves ~$2 / 7 bp on coupon-aligned calls,
    # and effective convexity is numerically sensitive near the call
    # boundary at a 1bp bump.
    "price_at_oas_25bps":          2.5,
    "price_at_oas_50bps":          2.5,
    "price_at_oas_100bps":         2.5,
    "oas_bps_at_market":           10.0,
    "effective_duration_at_oas":   1.5,
    "effective_convexity_at_oas":  500.0,
    # Tier 5.2.4 — independent calibration parity. Mean reversion is held
    # fixed on both sides at 0.03, so a-tolerance is exact. σ tolerance is
    # 1e-4 absolute: observed Rust-vs-QL σ residual is 1.6e-5 worst-case
    # (Ford, sparse 2-helper strip), well within margin.
    "hw1f_a_calibrated":           1e-12,
    "hw1f_sigma_calibrated":       1e-4,
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


SNAPSHOTS = [
    {"label": "2025-12-31", "convex": "convex.csv", "ql": "ql.csv"},
    {"label": "2025-06-30", "convex": "convex_20250630.csv", "ql": "ql_20250630.csv"},
]


def _diff_snapshot(label: str, convex_path: pathlib.Path, ql_path: pathlib.Path):
    """Diff one (convex, ql) CSV pair. Returns (per_bond, pass, fail, missing,
    convex_count, ql_count). Returns None if both files are missing."""
    if not convex_path.exists() and not ql_path.exists():
        return None
    convex = load(convex_path) if convex_path.exists() else {}
    ql = load(ql_path) if ql_path.exists() else {}

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

    return per_bond, pass_count, fail_count, missing, len(convex), len(ql)


def _render_snapshot(label: str, per_bond, pass_count, fail_count, missing,
                     convex_count, ql_count) -> list[str]:
    lines: list[str] = []
    lines.append(f"# Snapshot: {label}\n")
    lines.append(f"Convex rows: {convex_count}  ")
    lines.append(f"QuantLib rows: {ql_count}  ")
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

    # Detail per bond
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

    return lines


def main() -> int:
    total_pass = 0
    total_fail = 0
    sections: list[str] = []
    rendered_any = False

    for snap in SNAPSHOTS:
        result = _diff_snapshot(snap["label"], HERE / snap["convex"], HERE / snap["ql"])
        if result is None:
            continue
        per_bond, p, f, m, cc, qc = result
        total_pass += p
        total_fail += f
        sections.extend(_render_snapshot(snap["label"], per_bond, p, f, m, cc, qc))
        rendered_any = True

    if not rendered_any:
        print("reconcile: no snapshots produced output", file=sys.stderr)
        return 1

    header = [
        "# Convex ↔ QuantLib reconciliation report\n",
        f"**Total passes: {total_pass}**  ",
        f"**Total fails: {total_fail}**  ",
        f"Snapshots: {sum(1 for s in SNAPSHOTS if (HERE / s['convex']).exists())}",
        "",
    ]

    out = HERE / "reconciliation_report.md"
    out.write_text("\n".join(header + sections), encoding="utf-8")
    print(f"reconcile: wrote {out}", file=sys.stderr)
    print(f"reconcile: {total_pass} pass / {total_fail} fail", file=sys.stderr)
    return 1 if total_fail else 0


if __name__ == "__main__":
    sys.exit(main())
