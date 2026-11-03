# convex-wasm

The Convex fixed income library compiled to WebAssembly, for browsers and Node.js.

It runs the same Rust engine as the Rust crate, the C ABI and the Excel add-in,
entirely on the client. There is no server to call. The main entry point is
`library()`, which takes a JSON request and returns a JSON response. A few
simpler bond functions sit next to it. Live demo, built on this package:
<https://sujitn.github.io/convex/>

## Install

Each [GitHub release](https://github.com/sujitn/convex/releases) attaches two npm
tarballs. Pick the one that matches where you run the code:

- `convex-wasm-web-v<version>.tgz`: ES module with an async init, for browsers and bundlers.
- `convex-wasm-nodejs-v<version>.tgz`: CommonJS that loads synchronously, for Node.js.

```sh
npm install ./convex-wasm-web-v0.14.0.tgz
```

Both are named `convex-wasm`. To build from source, install
[wasm-pack](https://rustwasm.github.io/wasm-pack/) and run, from the repo root:

```sh
wasm-pack build crates/convex-wasm --target web --release     # or --target nodejs
```

## Browser and Node.js

The web build must be initialised once, before any call:

```js
import init, { library } from "convex-wasm";

await init();
const envelope = JSON.parse(library(JSON.stringify(request)));
```

`init()` fetches the `.wasm` file next to the module. To supply the URL or the
bytes yourself, use `await init({ module_or_path: urlOrBytes })`. The Node.js
build needs no init: `const { library } = require("convex-wasm");`

## library()

`library(request_json)` takes a string and returns a string. A request has a
`schema_version`, an `inputs` block (calendars, curves, fixings and so on) and an
`operation` that says what to do. This one takes a two-year zero coupon bond with
a clean price of 97 per 100 and asks for its yield:

```js
const request = {
  schema_version: 1,
  inputs: { calendars: [{ pin: { id: "weekend.synthetic", version: 1 }, weekend_days: [5, 6], holidays: [] }],
    calibration: null, curves: [], fixings: [], helper_calendars: [], publications: [] },
  operation: {
    kind: "zero",
    input: {
      calendar: { id: "weekend.synthetic", version: 1 },
      compounding: "semi_annual",
      currency: "USD",
      day_count: "Act365Fixed",
      face_value: "1000",
      identifiers: { cusip: null, figi: null, isin: null, issue_name: null, issuer_name: null, ticker: "example.zero" },
      issue_date: "2025-01-01",
      issue_price: "94.12345678901234567890123456",
      maturity: "2027-01-01",
      redemption_value: "1050",
      settlement_days: 1,
    },
    action: { kind: "quote_price", clean_price_per100: "97", settlement: "2025-01-01" },
  },
};

const envelope = JSON.parse(library(JSON.stringify(request)));
if (envelope.ok === "true") {
  console.log(envelope.result.data.pricing.ytm_decimal); // 0.04001981995568643
} else {
  console.error(envelope.error.code, envelope.error.message);
}
```

The response is always an envelope. `ok` is the string `"true"` or `"false"`, not
a boolean. On success the answer is in `result.data`. On failure `error` has a
`code`, a `message` and sometimes `details`. A malformed request comes back as
`invalid_input`; it does not throw.

Exact decimal values, such as face value, cash amounts and the issue price above,
are JSON strings in both directions, so they never pass through a JavaScript
`Number`. Computed metrics like yield and duration are ordinary numbers, and
rates are decimals (0.04 is 4%).

`library_schema()` returns the JSON Schema for requests. Operations and their
fields are listed in
[docs/LIBRARY-API.md](https://github.com/sujitn/convex/blob/main/docs/LIBRARY-API.md),
and `tests/fixtures/library/cases.json.xz` has about a hundred working requests
(`python3 tools/fixtures.py materialize` unpacks it).

## Convenience functions

These take and return plain JavaScript objects, not JSON strings. Bond terms go in
as one params object. Failures come back in an `error` field instead of throwing.

| Function | What it does |
|---|---|
| `price_from_yield(params, target_ytm)` | Clean, dirty and accrued price from a yield. Prices are per 100 face; the yield is in percent. |
| `price_from_spread(params, target_spread_bps, curve_points)` | Price from a Z-spread over a curve. Spread in basis points, prices per 100 face. |
| `analyze_bond(params, clean_price, curve_points)` | Yield, duration, convexity and spreads from a per-100 clean price. |
| `calculate_accrued(params)` | Accrued interest per 100 of original face. |
| `calculate_simple_metrics(params, clean_price)` | Accrued, dirty price and current yield (in percent) from a clean price. |
| `get_cash_flows(params)` | Future cash flows, in the bond's currency. |
| `get_convention_options()` | Markets, instrument types and yield conventions, for dropdowns. |
| `get_default_conventions(market, instrument_type)` | Default day count, compounding and settlement days for a market. |

```js
import { price_from_yield } from "convex-wasm";

const bond = {
  coupon_rate: 5.0, // percent
  maturity_date: "2035-06-15", issue_date: "2025-06-15", settlement_date: "2026-11-02",
  frequency: 2, day_count: "30/360", currency: "USD",
};

price_from_yield(bond, 4.5);
// { clean_price: 103.533..., dirty_price: 105.435..., accrued_interest: 1.9027... }
```

## Size and loading

The release build's `.wasm` file is 13.3 MB (13,278,312 bytes) uncompressed, so
serve it with gzip or brotli. Downloading and compiling it takes a moment; load it
in a Web Worker to keep the page responsive. The app in
[www/src/engine/](https://github.com/sujitn/convex/tree/main/www/src/engine)
does this: the worker fetches the file, calls `init`, then answers `library()`
requests in order. A panic in wasm32 aborts, so if a call throws, discard that
worker and start a new one.

## License

MIT. The engine includes CDS code based on the ISDA CDS Standard Model, whose
public license ships in the package as LICENSE-ISDA-CDS-STANDARD-MODEL.txt.

This application is based on the ISDA CDS Standard Model (version 1.8.3), developed and supported in collaboration with Markit Group Ltd.
