# Convex Terminal

A browser workstation for bond analytics. Every analytic (price, yield, spread,
risk, curve, schedule) comes from the Convex library, compiled to WebAssembly and
called through its strict supplied-input API, `library(request_json)`. Market data
comes straight from free public sources or from you, and is shown as supplied;
holiday tables are generated in the browser. Nothing is fabricated when a source
or calculation fails.

Live: <https://sujitn.github.io/convex/>

## Functions

| Code | Screen | Library operations |
| --- | --- | --- |
| `MKT` | Market data: Treasury par curve and bill rates, Treasury auction results, NY Fed reference rates and SOFR history, ECB euro-area spot curves and €STR, Bank of Canada benchmark yields and CORRA. Curve values are editable, tenors can be added and whole tables pasted; edits are marked and kept in this browser (rates and auction results are shown as published). **Your data** holds the curves and bonds you supply and exports/imports everything you entered. | `calibration.build` |
| `CRV` | Curve workbench: native par-coupon calibration (UST, Canada, your par curves), OIS calibration from your swap rates, supplied spot nodes (ECB, your zero curves), zero/forward/discount sampling, point queries, calibration repricing table, cross-curve comparison. | `calibration.build`, `curves`, `curve_query` |
| `SEC` | Security description: actual Treasuries from auction results, editable examples (callable, step-up, sinking fund, euro), bonds you import from a spreadsheet, full `BondTerms`, the engine-generated schedule. | `bond.cashflows` |
| `YAS` | Yield and spread: quote by price (decimal or 32nds), yield, Z-spread or G-spread. Treasuries open on their curve-implied price. ICMA yield and four other conventions, current and simple yield, Z-spread over the government curve or a curve you choose (Z-spread to swaps over an OIS curve), G-spread against actual benchmark bonds (the on-the-run Treasuries or bonds you pick, in any currency you have bonds in), interpolated by maturity or the same-maturity bond, at curve-implied, imported or typed prices, duration, convexity, DV01, CS01, invoice, ±100bp ladder, workouts for callables. | `icma_quotation`, `bond.price`, `bond.yield_inverse`, `bond.spread_inverse`, `government_spread`, `government_spread_price`, `cash_security.invoice`, `yield_scalar`, `workout` |
| `CFL` | Cash flows with native discount factors on the chosen curve; the PV column reconciles to the engine price. | `bond.cashflows`, `curve_query`, `bond.price` |
| `SCN` | Thirteen government-curve scenarios and par-quote key-rate durations, each by full recalibration and repricing at constant Z-spread. | `calibration.build`, `bond.price` |
| `OAS` | Callables on a Hull-White one-factor tree over the chosen discount curve: OAS, option value, effective duration and convexity, OAS by volatility, workouts, exercise schedule, tree convergence. | `optional_bond.*`, `workout` |
| `FRN` | SOFR floating-rate notes, compounded in arrears (lookback, observation shift or lag, lockout, payment delay): quote by clean price or discount margin, accrued from the published NY Fed fixings, the current coupon's fixings and projected days, every coupon marked fixed, partly fixed or projected, DM01 and spread duration (DM ± 1bp), IR01 and rate duration (SOFR curve ± 1bp, coupons re-projected), a DM ladder, and the DM to each call date for callable notes. Your own notes from a terms form or a pasted table. | `bond.construct`, `bond.price`, `bond.spread_inverse`, `floating_workout` |
| `CDS` | Credit default swaps: credit curves from par spreads you supply (entity, seniority, recovery, restructuring clause, 6M–10Y), natively calibrated to a survival curve on your SOFR/€STR swap curve or, labelled, the government curve; repricing table, survival, default probability and hazard by tenor; a standard contract (roll-date maturity, 100/500bp coupon, buy or sell protection) with points upfront, cash amount, par spread, legs, RPV01, CS01 overall and by tenor, recovery and discount sensitivity, the premium leg, and spread ↔ upfront conversion. | `calibration.build`, `curve_query`, `cds.price`, `cds.convert`, `credit_risk.sensitivity`, `credit_risk.scenario` |
| `BILL` | Treasury bills on the official convention: discount rate, investment rate and price from each other, and the daily bill curve. | `zero.treasury_bill` |
| `CAL` | The rule-generated holiday tables each request carries, and a settlement calculator. | — |
| `API` | A console for `library()` with templates built from the current security and curve, and a browser for the generated request schema. | any |

Every call is logged in the inspector (`<>` in the top bar) with its exact
request and response, timing, and a button to edit it in `API`; a batch keeps
every member, stepped through one at a time.

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
`node scripts/refresh-snapshot.mjs`. The proxy-only UK and Japan sources are not
in the snapshot. Provenance (live, snapshot, edited,
derived) and the as-of date are shown with the data.

## Your data

There is no free source of swap rates, bond reference data beyond Treasuries,
or individual bond prices, so the terminal takes them from you. Everything below
accepts a paste from Excel (tab-separated, decimal commas included), a CSV file
or a terminal export. Under `MKT` → Your data:

- **Curves you supply.** Three kinds, each built by the engine:
  - *OIS swap rates* for SOFR, €STR, SONIA or CORRA. Each tenor is one
    spot-starting par swap, annual fixed against compounded overnight, on the
    index's calendar, spot lag, payment delay and day count (all editable). The
    native rate calibration fits a Discount-role curve with an explicitly
    declared zero-basis overnight projection; every swap reprices to about 1e-10.
    The request carries one publication record per overnight observation date,
    and one request admits at most 4,096 items per array; the app stops at 4,000
    to leave room, so a single fit reaches about 15 years. Supply longer points
    as a zero curve. The calendar follows the index.
  - *Par yields* in USD, EUR, GBP or CAD: the same par-coupon calibration as
    the Treasury curve, with your frequency, day count and settlement lag.
  - *Zero rates*, continuously compounded, used directly as nodes in either the
    collateral (swap) or government role, any horizon.

  Pick any curve in the security's currency as the spread curve on `YAS`, `CFL`
  and `OAS`. Over a Discount-role curve the bond's Z-spread is priced in the
  collateral role: the Z-spread to swaps. G-spread and `SCN` stay on the
  government curve.
- **Tables.** Every curve grid takes added tenors and pasted tables (two
  columns, tenor then rate in percent, from Excel, a terminal or a web page);
  "Copy template" gives the tenor column to fill in a sheet. The parser reports
  lines it can't read instead of guessing.
- **Bonds.** Paste rows with a header line naming the columns, in any order:
  CUSIP or ISIN, name, coupon, maturity, issue/dated/first coupon dates,
  currency, frequency, day count, settlement days, redemption, clean price
  (decimal or 32nds) and calls (`date@price; date@price`). Coupon and maturity
  are required; anything else missing gets the currency's market default, and
  the preview lists each default applied before you add the bonds. Dates may be
  ISO, US or day-first (detected, or chosen), `15-Mar-2034` or Excel serials.
  Without an issue date a bond is taken as regular, accruing from its last
  coupon date. A pasted price becomes the bond's opening quote on `YAS`.
  "Copy template" puts the header and the current on-the-run Treasuries on the
  clipboard as a starting sheet.
- **Benchmark prices.** On `YAS`, type actual prices for the on-the-run
  Treasuries; G-spread uses yours and labels them.
- **Securities.** On `SEC`, copy any bond as a custom security and edit its terms.
- **Export and import.** Everything above, and the spread curve chosen for
  each currency, downloads as one JSON file and imports back (merging) in
  another browser; malformed records are skipped and counted.

Example swap rates can be filled in to try the OIS route; they are hypothetical
and the curve is marked "Example rates" until you change them.

## Floating-rate notes

`FRN` prices SOFR floaters the way US corporates and agencies issue them:
daily SOFR compounded in arrears over the interest period, the margin added
outside the compounding, Act/360, quarterly by default. The example note is
hypothetical; create your own on `FRN` (terms form) or paste rows on `SEC` with
an `Index` column (`SOFR`), `Spread` in basis points and an optional `Lookback`
in business days; a table of floaters needs no Coupon column. Every request carries:

- **Terms**: full `BondTerms` with an `overnight` coupon (lookback, observation
  shift, lockout), payment delay and Modified Following periods, all stated.
  Profiles cover the adjusted accrual span, as the engine requires.
- **Fixings**: the NY Fed SOFR history (the last 140 fixings, about six months,
  so a semi-annual period is covered), each as a final observation published
  and known at 08:00 New York on the next business day. A fixing published by
  the end of the trade date must be there: if it is not, the engine reports it
  missing and the screen offers a reload. Published history is never projected.
- **Publications**: one expected publication per observation date from the
  first unpaid coupon to maturity; one request admits about 15 years of them.
- **Curves**: your SOFR OIS curve (`MKT` → Your data, or the example-rates
  button on `FRN`) as the collateral discount curve plus its zero-basis
  `OvernightProjection` view (`<curve>.ON`), the same view its calibration
  declares. Another discount curve can be chosen; the projection is then an
  assumed zero basis, and the screen says so.

Choices worth knowing:

- **Discount margin** is the engine's frozen-projection measure: an Act/365F
  continuous spread added to the collateral discount rate with every projected
  coupon held fixed. A note opens at a DM equal to its quoted margin (there is
  no price source) until you type a price or DM.
- **Accrued under an observation shift** needs an explicit end to the shifted
  observation weights. The app states settlement less the lookback on the
  observation calendar, the observation day that pairs with settlement; the
  margin accrues on interest-period days to settlement.
- **Risk** re-runs the engine: DM ± 1bp with coupons held, and the fitted SOFR
  zero curve ± 1bp in parallel (projection and discounting together, coupons
  re-projected) at the same DM.
- **Callable notes**: straight figures are to maturity with the calls removed;
  `floating_workout` gives the DM to each legal redemption date at the same
  price, a call inside a period paying the coupon accrued to it. No path is
  chosen as worst: the engine has no stochastic exercise model for floaters.
- **Not offered**: index floors and caps (the library has no overnight coupon
  option yet), term-rate indices, and rate rounding (coupons are unrounded).

## Credit default swaps

There is no free source of CDS quotes, so `CDS` takes par spreads from you
(typed or pasted, by tenor, in basis points), or fills a hypothetical example
set marked "Example spreads". Curves are kept in this browser.

- **Contracts** follow the standard conventions, all stated in the request:
  ISDA Standard Model (`Standard183`), quarterly Act/360 premiums with accrued
  paid on default, maturities on the 20 June/20 December roll (CDS2015
  semi-annual roll), a full first coupon from the last quarterly roll date,
  step-in T+1 and cash settlement T+3 business days on New York + London (USD)
  or TARGET2 + London (EUR). Recovery defaults to 40% senior, 25%
  subordinated; the restructuring clause (XR14, MM14, …) is a document pin and
  is not valued by the model.
- **Calibration**: one par CDS per tenor, quoted as a running spread,
  bootstrapped by the engine to a log-linear survival curve (one hazard rate per
  tenor, no extrapolation). The fitted grid is re-supplied as direct survival
  nodes, so pricing reuses exactly that curve.
- **Discounting**: the CDS pricer takes dated discount factors. They are queried
  from your swap curve (`MKT` → Your data, OIS) monthly and re-supplied; with no
  swap curve the government curve stands in, and the screen says so.
- **Quotes and risk**: points upfront and par spread are on the calibrated
  curve; the spread ↔ upfront converter is the conventional single flat hazard
  at the contract coupon, so the two upfronts differ unless the curve is flat.
  CS01 by tenor is the engine's signed central ±1bp per quote with the curve
  refitted; the overall CS01 moves every quote together. Recovery risk is shown
  with spreads held (hazard refitted) and with survival held.
- **Not offered**: bond–CDS basis. The engine's `bond_cds_hedge` admits a
  supplied bond/CDS mapping and residual basis but returns no basis or hedge
  ratio, and bonds here carry no issuer identity to map.

## What the engine is given

The library never looks anything up. Calendar-dependent requests carry:

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

- **G-spread benchmarks** are actual securities: by default the on-the-run
  Treasuries from auction results, or the bullet bonds you pick on `YAS`
  (Treasuries, examples or bonds you imported, in the target's currency). The
  engine either interpolates the benchmark yield between the two maturities
  bracketing the target or uses the bond maturing the same day; a spread to a
  single benchmark of a different maturity is not an engine measure and is not
  shown. Benchmark prices are curve-implied from the fitted government curve
  (labelled) unless you typed one or the bond was imported with one; there is
  no free intraday source of individual bond prices.
- **Default quote**: an actual Treasury opens on its curve-implied clean price,
  rounded to the nearest 1/256, so Z and G read as results. Typing a price
  replaces it; an untouched one follows curve rebuilds.
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
  used is shown. Some curve shapes, such as an OIS curve with a dense, inverted
  short end, fail the engine's tree refinement check for some bonds; the
  `no_convergence` error is shown, and the inspector has each grid's price.
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
npm test                  # startup admission, market parsers, paste and data import (offline)
npm run build:wasm:node   # wasm-pack --target nodejs → .engine-node/
npm run test:engine       # every request builder against the real engine
```

The engine runs in a Web Worker (`src/engine/worker.js`); a trap restarts the
worker instead of leaving the page on a broken instance. If the engine cannot
load, the app shows an error and does not mount.
