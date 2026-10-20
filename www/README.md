# Convex Terminal

A browser workstation for bond analytics. Every number on screen comes from the
Convex library, compiled to WebAssembly and called through its strict
supplied-input API, `library(request_json)`. Market data comes straight from free
public sources; nothing is fabricated when a source or calculation fails.

Live: <https://sujitn.github.io/convex/>

## Functions

| Code | Screen | Library operations |
| --- | --- | --- |
| `MKT` | Market data: Treasury par curve and bill rates, Treasury auction results, NY Fed reference rates and SOFR history, ECB euro-area spot curves and €STR, Bank of Canada benchmark yields and CORRA. Every value is editable; edits are marked and kept in this browser. | — |
| `CRV` | Curve workbench: native par-coupon calibration (UST, Canada), supplied spot nodes (ECB), zero/forward/discount sampling, point queries, calibration repricing table, cross-curve comparison. | `calibration.build`, `curves`, `curve_query` |
| `SEC` | Security description: actual Treasuries from auction results, editable examples (callable, step-up, sinking fund, euro), full `BondTerms`, the engine-generated schedule. | `bond.cashflows` |
| `YAS` | Yield and spread: quote by price (decimal or 32nds), yield, Z-spread or G-spread. ICMA yield and four other conventions, current and simple yield, Z-spread, G-spread against actual on-the-run Treasuries, duration, convexity, DV01, CS01, invoice, ±100bp ladder, workouts for callables. | `icma_quotation`, `bond.price`, `bond.yield_inverse`, `bond.spread_inverse`, `government_spread`, `government_spread_price`, `cash_security.invoice`, `yield_scalar`, `workout` |
| `CFL` | Cash flows with native discount factors; the PV column reconciles to the engine price. | `bond.cashflows`, `curve_query`, `bond.price` |
| `SCN` | Thirteen curve scenarios and par-quote key-rate durations, each by full recalibration and repricing at constant Z-spread. | `calibration.build`, `bond.price` |
| `OAS` | Callables on a Hull-White one-factor tree: OAS, option value, effective duration and convexity, OAS by volatility, workouts, exercise schedule, tree convergence. | `optional_bond.*`, `workout` |
| `BILL` | Treasury bills on the official convention: discount rate, investment rate and price from each other, and the daily bill curve. | `zero.treasury_bill` |
| `CAL` | The rule-generated holiday tables each request carries, and a settlement calculator. | — |
| `API` | A console for `library()` with templates built from the current security and curve, and a browser for the generated request schema. | any |

Every call is logged in the inspector (`<>` in the top bar) with its exact
request and response, timing, and a button to edit it in `API`.

## Market data sources

| Source | Data | Browser access |
| --- | --- | --- |
| U.S. Treasury, daily par yield curve and bill rates (XML) | UST par curve, bill discount and coupon-equivalent rates | Direct |
| Treasury FiscalData, auctions | Actual securities: CUSIP, coupon, dates, auction price | Direct (blocks headless browsers) |
| Federal Reserve Bank of New York | SOFR, EFFR, OBFR, TGCR, BGCR, SOFR averages and history | Direct |
| European Central Bank Data Portal | AAA and all-issuer spot curves (continuously compounded), Svensson parameters, €STR | Direct |
| Bank of Canada Valet | Benchmark bond yields, CORRA | Direct |
| Bank of England, Japan MoF | SONIA and Bank Rate, JGB yields | Needs a CORS proxy you configure under `MKT` → UK and Japan |

A snapshot of every direct source is bundled (`src/market/snapshot.js`) so the
app works offline and before live data arrives; refresh it with
`node scripts/refresh-snapshot.mjs`. Provenance (live, snapshot, edited,
derived) and the as-of date are shown with the data.

## What the engine is given

The library never looks anything up. Each request carries:

- **Calendars**: full holiday tables generated from published rules
  (`src/lib/calendars.js`: SIFMA, Fedwire, TARGET2, England & Wales, Canada,
  2000–2080), pinned by rule set and span.
- **Contract terms**: complete `BondTerms` built from the security model
  (`src/lib/terms.js`). Coupons, face and prices are exact decimal strings made
  by moving the decimal point in the typed digits, never by float arithmetic.
- **Curves**: par yields are calibrated by the engine from one nominal par bond
  per tenor (`ParCouponRate` helpers, piecewise, linear continuous zeros). The
  calibrated node grid is then re-supplied as a direct curve with the same
  representation, so later requests reuse exactly the fitted curve. ECB spot
  rates are continuously compounded per the ECB's technical notes and are used
  as nodes directly.

## Choices worth knowing

- **G-spread benchmarks** are the actual on-the-run Treasuries from auction
  results, but their quotes are curve-implied prices from the fitted Treasury
  curve (labelled "Curve-implied"); there is no free intraday source of
  individual Treasury prices.
- **Yields**: the headline yield is the ICMA contextual yield (Act/Act period
  time, the street-equivalent convention). Dated-cash yields are shown with
  their explicit day count; Act/Act ICMA is not a context-free dated-cash basis,
  so Treasuries use Act/Act ISDA there, labelled.
- **Duration and convexity** in `YAS` are from engine valuations at the ICMA
  yield ±1bp. Key-rate durations in `SCN` bump one par quote at a time and
  recalibrate.
- **Callables**: straight analytics run with the calls removed and say so;
  workouts price every legal path; OAS uses Hull-White parameters you choose,
  sent as an explicitly assumed model. If the bumped trees fail the engine's
  refinement check at ±50bp, effective risk is retried at ±100bp and the shift
  used is shown.
- **Errors** from the engine are shown verbatim with their code. A missing
  spread is shown as missing, never as zero.

## Run it

```sh
npm ci
npm run build:wasm        # wasm-pack --target web → pkg/
npm start                 # http://localhost:3000
```

Tests:

```sh
npm test                  # startup admission, market parsers (offline)
npm run build:wasm:node   # wasm-pack --target nodejs → .engine-node/
npm run test:engine       # every request builder against the real engine
```

The engine runs in a Web Worker (`src/engine/worker.js`); a trap restarts the
worker instead of leaving the page on a broken instance. If the engine cannot
load, the app shows an error and does not mount.
