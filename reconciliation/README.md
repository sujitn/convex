# Convex ↔ QuantLib reconciliation

A reconciliation test harness comparing Convex's pricing and risk output against QuantLib's on a mixed book of real fixed-income instruments as of **2025-12-31**.

## Why

Convex is a Rust fixed-income library with Bloomberg-YAS ambitions. A side-by-side test against QuantLib — the most widely-used open-source quant library — is the cheapest way to surface convention bugs, numerical drift, and edge-case handling differences before they hit a real book.

## What's in the book

13 instruments spanning the convention space. Every identifier is a real, traceable CUSIP/ISIN except the one clearly-labelled synthetic. Full source citations in `book.json`.

| ID | Issuer | Coupon | Maturity | Conv | Purpose |
|---|---|---|---|---|---|
| `UST_10Y` | US Treasury | 4.000 | 2035-11-15 | SA, ACT/ACT ICMA | baseline OTR 10Y |
| `UST_30Y` | US Treasury | 4.625 | 2055-11-15 | SA, ACT/ACT ICMA | long end |
| `UST_5Y_short` | US Treasury | 3.875 | 2027-12-31 | SA, ACT/ACT ICMA | short-dated (2Y remaining) |
| `UST_FRN_2Y` | US Treasury | 13W-Bill + 19bps | 2027-10-31 | ACT/360 simple | FRN / index-linked coupons |
| `UST_TIPS_10Y` | US Treasury | 1.875 real | 2035-07-15 | SA, ACT/ACT ICMA | inflation-linked |
| `UK_GILT_10Y` | UK DMO | 4.750 | 2035-10-22 | SA, ACT/ACT ICMA | GBP sovereign |
| `DE_BUND_10Y` | German Finanzagentur | 2.500 | 2035-02-15 | **Annual**, ACT/ACT ICMA | exercises non-semi-annual |
| `JP_JGB_10Y` | Japan MoF | 1.700 | 2035-09-20 | SA, ACT/ACT ICMA | JPY / different basis |
| `AAPL_4_65_2046` | Apple | 4.650 | 2046-02-23 | SA, 30/360 US | US corp 30/360 path |
| `MSFT_3_5_2035` | Microsoft | 3.500 | 2035-02-12 | SA, 30/360 US | US corp 30/360 path |
| `VZ_4_329_2028` | Verizon | 4.329 | 2028-09-21 | SA, 30/360 US | US corp mid-curve |
| `F_6_798_2028` | Ford Motor Credit | 6.798 | 2028-11-07 | SA, 30/360 US | IG callable (MW T+35, par-call 2028-10-07) — **fully verified** |
| `SYNTH_HY_STEPDOWN_01` | synthetic | 7.500 | 2030-04-15 | SA, 30/360 US | multi-date step-down call schedule |

## Metrics under test (per instrument)

- Clean price from yield
- Dirty price from yield
- Accrued interest
- Yield-to-maturity from clean price
- Macaulay duration
- Modified duration
- Convexity
- DV01
- For callables: price under the call schedule (yield-to-worst analogue)
- For FRN: projected coupons and discount margin
- For TIPS: real-yield vs nominal-yield distinction, inflation-linked accrued

## Tolerances (initial proposal)

| Metric | Tolerance |
|---|---|
| Clean / dirty price (per 100) | `1e-6` |
| Accrued interest (per 100) | `1e-8` |
| YTM (decimal) | `1e-7` (0.001 bp) |
| Macaulay / modified duration (years) | `1e-4` |
| Convexity | `1e-3` |
| DV01 (per 100 face) | `1e-7` |

Anything outside tolerance gets investigated. Some deltas will be legitimate convention-interpretation differences (e.g. stub-period handling in the first coupon) — those get documented and whitelisted.

## Curves

Primary curve for USD discount is **US Treasury constant-maturity yields on 2025-12-31** (see `curves.json`). This is a reproducible, free, primary-source curve.

### Why not SOFR OIS?

A bank-quality USD discount curve would be SOFR OIS. Those quotes are paywalled (ICE Swap Rate / Bloomberg / TraditionData). For a reconciliation test, what matters is **same input → both libraries produce same output** — the curve doesn't need to be bank-quality, just consistent. UST CMT is defensible, free, and identical on both sides.

The SOFR overnight fixing series (2024-01-01 → 2025-12-31) **is** free from NY Fed and is pulled at harness time for FRN reconstruction.

### What's pinned vs pulled at runtime

Pinned in `curves.json`:
- 6M, 1Y, 2Y, 3Y, 5Y, 10Y, 20Y, 30Y UST CMT (from Advisor Perspectives year-end snapshot)
- SOFR fixing on 2025-12-31 (3.87%, year-end turn)

Pulled live by `pull_market_data.py`:
- 1M, 3M, 7Y UST CMT (FRED CSV)
- Full SOFR daily series 2024–2025 (NY Fed JSON)
- TIPS index ratio on 2025-12-31 (TreasuryDirect)

## Known limits

1. **Apple / MSFT / Verizon make-whole spreads unknown** from open sources. Their 424B prospectuses contain the spread in bps; fetches were blocked in the research environment. For the reconciliation we treat them as bullet bonds — the make-whole is deep out-of-the-money in any reasonable rate scenario, so this has zero numerical impact on the listed metrics. If we later want to test make-whole-specific valuations, open the 424B2 filings and add the spreads.
2. **JGB #380 ISIN** was pattern-extrapolated then independently confirmed via Cbonds (`JP1103801RA7`). The canonical authority is JSDA; re-confirm if reliability matters more than the test scope.
3. **HY step-down call schedule** is **synthetic** (`SYNTH_HY_STEPDOWN_01`). Real HY indenture PDFs were 403 in this environment. Three candidate real bonds were identified (T-Mobile 3.875% 2030, Occidental 8.875% 2030, Bausch 11% 2028) — if real HY coverage matters, open one of those exhibits from EDGAR and replace the synthetic.
4. **UK / EU / JP discount curves** are placeholders in `curves.json`. Primary free sources identified (BoE, ECB, MoF Japan) — pull URLs documented, but the actual fixed-tenor quotes aren't in the file yet.

## Milestones

- **[x] Milestone 1: Book + curves assembled** — this commit. `book.json`, `curves.json`, `pull_market_data.py`, `README.md`.
- **[ ] Milestone 2: Benches** — `convex_bench/` (Rust binary), `ql_bench.py` (QuantLib), `reconcile.py` (diff).
- **[ ] Milestone 3: First reconciliation run + triage** — run both sides, classify every delta as bug / convention-drift / acceptable, fix bugs, whitelist documented drifts.
- **[ ] Milestone 4: CI integration** — run on every pricing-code change, block merge on new deltas.

## Layout

```
reconciliation/
├── README.md                    # this file
├── book.json                    # 13 instruments with full terms and citations
├── curves.json                  # discount curve inputs as of 2025-12-31
├── pull_market_data.py          # fetch dynamic pieces (FRED, NY Fed, TreasuryDirect)
├── sofr_fixings.csv             # generated by pull_market_data.py
├── ust_cmt_20251231.csv         # generated by pull_market_data.py
└── tips_index_ratio_20251231.json  # generated by pull_market_data.py
```
