# Market Data Sources

Primary-source provenance for `curves.json`, `sofr_fixings.csv`, and
`tips_index_ratio_20251231.json`. Values are regeneratable via
`pull_market_data.py`.

Valuation date: **2025-12-31**.

## UST CMT (`UST_CMT`)

Treasury constant-maturity yields from FRED series `DGSxx`. CSV endpoint:
`https://fred.stlouisfed.org/graph/fredgraph.csv?id=DGS1MO,DGS3MO,…,DGS30&cosd=2025-12-31&coed=2025-12-31`.
Puller: `pull_ust_cmt()` → `ust_cmt_20251231.csv`. Values in `curves.json`
were mirrored from the primary source in an earlier session; `DGS7` was
not present in that snapshot and is interpolated between `DGS5` and `DGS10`
at read time.

## SOFR (`SOFR_OIS`)

NY Fed SOFR daily fixings. Used to reconstruct the UST FRN projection.
No free public SOFR-OIS swap curve; `UST_CMT` is the documented discount-curve
proxy for USD. Puller: `pull_sofr_fixings()` → `sofr_fixings.csv` (499 rows,
2024-01-01 through 2025-12-31).
2025-12-31 overnight fixing: **3.87%** (elevated vs. normal 3.70–4.00% — year-end turn).

## UK Gilt Nominal Spot (`UK_GILT_CURVE`)

Bank of England nominal spot curve. Published as xlsx only.

* Archive: <https://www.bankofengland.co.uk/-/media/boe/files/statistics/yield-curves/glcnominalddata.zip>
* Workbook: `GLC Nominal daily data_2025 to present.xlsx`
* Sheet: `4. spot curve`

BoE publishes **continuously-compounded** spot yields; `curves.json`
stores the semi-annual equivalent (`r_sa = 2·(exp(r_c/2)−1)`) for
consistency with `UST_CMT`. Both columns are in `uk_gilt_20251231.csv`.

Puller: `pull_uk_gilt()` (needs `openpyxl`).

## ECB Euro-Area AAA Spot (`DE_BUND_CURVE`)

ECB SDMX series `YC.B.U2.EUR.4F.G_N_A.SV_C_YM.SR_<tenor>Y`. The ECB publishes
Svensson-model spot yields fit to the AAA-rated euro-area government bond
universe — this is used as the EUR discount curve rather than a Bund-only
curve because the AAA curve is the ECB's primary daily publication.

Endpoint: `https://data-api.ecb.europa.eu/service/data/YC/B.U2.EUR.4F.G_N_A.SV_C_YM.SR_1Y+SR_2Y+…+SR_30Y?startPeriod=2025-12-31&endPeriod=2025-12-31&format=csvdata`.
Continuous → semi-annual conversion as for UK. Raw and converted values in
`ecb_aaa_20251231.csv`.

Puller: `pull_ecb_aaa()`.

## JGB Par Yields (`JP_JGB_CURVE`)

Japan MOF `jgbcme_all.csv`. Par yields at fixed tenors, semi-annual market
convention; stored as-is in `curves.json`.

Endpoint: <https://www.mof.go.jp/english/policy/jgbs/reference/interest_rate/historical/jgbcme_all.csv>.

Japan markets close 2025-12-31 for year-end holiday, so the last observation
is **2025-12-30** (`observation_date` in curves.json reflects this). The puller
walks December 2025 rows with proper date parsing — lexicographic compare
would rank "12/9" above "12/30".

Puller: `pull_jgb()` → `jgb_2025-12-30.csv` + `jgb_eoy2025.csv`.

## TIPS Index Ratio

CUSIP `91282CNS6`. Puller: `pull_tips_index_ratio()` captures the
TreasuryDirect search payload as `tips_search_raw.json`; the daily index-ratio
time series lives on the TIPS/CPI detail page and requires a manual pull
from <https://www.treasurydirect.gov/auctions/announcements-data-results/tips-cpi-data/tips-cpi-detail/?cusip=91282CNS6>.
Tier 2.2 work will complete this.

## Refresh

```bash
pip install openpyxl        # optional, only pull_uk_gilt needs it
python reconciliation/pull_market_data.py
# diff the resulting *.csv files against curves.json and hand-merge
```

FRED is blocked from some networks (TLS read timeout on
`fred.stlouisfed.org`); the UST CMT pull is the only one known to fail this
way. All other sources are reliable from a standard network.
