# convex-ffi

C-ABI FFI for the Convex fixed-income analytics library.

The boundary is JSON. Construction takes a `*Spec` JSON string and returns
an opaque `u64` handle. Analytics take a request JSON, return a response
JSON. Adding a new bond shape, spread family, or pricing convention is a
serde enum variant plus a dispatch arm in `convex-analytics` /
`convex-ffi::dispatch` — **no new C symbol** has to ship.

## Building

```bash
cargo build --release -p convex-ffi
```

Produces:

| Artifact | Linux | macOS | Windows |
|---|---|---|---|
| Shared library | `libconvex_ffi.so` | `libconvex_ffi.dylib` | `convex_ffi.dll` |
| Static archive | `libconvex_ffi.a`  | `libconvex_ffi.a`   | `convex_ffi.lib` |
| Rust rlib | `libconvex_ffi.rlib` (for integration tests) |

## C surface (13 symbols)

### Construction

```c
uint64_t    convex_bond_from_json(const char* spec_json);
uint64_t    convex_curve_from_json(const char* spec_json);
const char* convex_describe(uint64_t handle);   // free with convex_string_free
void        convex_release(uint64_t handle);
int32_t     convex_object_count(void);
const char* convex_list_objects(void);          // free with convex_string_free
void        convex_clear_all(void);
```

`convex_bond_from_json` and `convex_curve_from_json` return `0` on failure;
read `convex_last_error()` for diagnostics.

### Stateless analytics (JSON in / JSON out)

```c
const char* convex_price        (const char* request_json);
const char* convex_risk         (const char* request_json);
const char* convex_spread       (const char* request_json);
const char* convex_cashflows    (const char* request_json);
const char* convex_curve_query  (const char* request_json);
```

All five return a heap-allocated UTF-8 string the caller MUST free with
`convex_string_free`. The body is a JSON envelope:

```json
{ "ok": "true",  "result": { ... } }
{ "ok": "false", "error": { "code": "...", "message": "...", "field": "..." } }
```

Error codes:

| Code | Meaning |
|---|---|
| `invalid_input` | Bad JSON, missing required field, or unparseable mark. May carry a `field` pointer. |
| `invalid_handle` | Handle not in the registry, or wrong kind for the call. |
| `analytics` | Solver did not converge, settlement ≥ maturity, etc. |

### Utilities

```c
const char* convex_schema(const char* type_name);  // free with convex_string_free
const char* convex_mark_parse(const char* text);   // free with convex_string_free
const char* convex_last_error(void);               // borrowed; do NOT free
const char* convex_version(void);                  // static
void        convex_clear_error(void);
void        convex_string_free(const char* s);
```

`convex_schema` accepts: `"Mark"`, `"BondSpec"`, `"CurveSpec"`,
`"PricingRequest"`, `"PricingResponse"`, `"RiskRequest"`, `"RiskResponse"`,
`"SpreadRequest"`, `"SpreadResponse"`, `"CashflowRequest"`,
`"CashflowResponse"`, `"CurveQueryRequest"`, `"CurveQueryResponse"`.

## DTOs

The complete request / response shapes are defined in
[`convex_analytics::dto`](../convex-analytics/src/dto.rs). Highlights:

### `BondSpec` (tagged JSON `"type"`)

```json
{ "type": "fixed_rate", "cusip": "037833100",
  "coupon_rate": 0.05, "frequency": "SemiAnnual",
  "maturity": "2035-01-15", "issue": "2025-01-15",
  "day_count": "Thirty360US", "currency": "USD", "face_value": 100 }

{ "type": "callable", ...fixed_rate fields...,
  "call_schedule": [{ "date": "2030-01-15", "price": 102.0 }],
  "call_style": "american" }

{ "type": "floating_rate", "spread_bps": 75, "rate_index": "sofr",
  "frequency": "Quarterly", "day_count": "Act360", ... }

{ "type": "zero_coupon", "compounding": "SemiAnnual",
  "day_count": "ActActIcma", ... }

{ "type": "sinking_fund", ...fixed_rate fields...,
  "schedule": [{ "date": "2031-01-15", "amount": 20.0, "price": 100.0 }] }
```

### `CurveSpec` (tagged JSON `"type"`)

```json
{ "type": "discrete", "ref_date": "2025-01-15",
  "tenors": [0.5, 1, 2, 5, 10, 30],
  "values": [0.045, 0.046, 0.047, 0.048, 0.049, 0.050],
  "value_kind": "zero_rate", "interpolation": "linear",
  "day_count": "Act365Fixed", "compounding": "Continuous" }

{ "type": "bootstrap", "ref_date": "2025-01-15",
  "method": "global_fit",
  "instruments": [
    { "kind": "deposit", "tenor": 0.25, "rate": 0.0525 },
    { "kind": "swap",    "tenor": 5.0,  "rate": 0.0425 }
  ],
  "interpolation": "linear", "day_count": "Act360" }
```

### `Mark` — accepted as text or tagged JSON

```text
99.5            ' clean price (default)
99.5C / 99.5D   ' explicit clean / dirty
99-16 / 99-16+  ' Treasury 32nds
4.65% / 4.65%@SA ' yield + frequency
+125bps@USD.SOFR ' Z-spread (default) over benchmark
125 OAS@USD.TSY  ' explicit spread type
```

### `PricingRequest`

```json
{ "bond": 100, "settlement": "2025-04-15",
  "mark": "99.5C",
  "curve": null,                   /* required for spread marks and FRNs */
  "quote_frequency": "SemiAnnual",
  "forward_curve": null            /* FRN projection curve */
}
```

### `SpreadRequest.params`

| Field | Used by | Meaning |
|---|---|---|
| `volatility` | OAS | Short-rate volatility, decimal (0.01 = 1%). |
| `forward_curve` | DM | Projection curve handle. Defaults to discount curve. |
| `current_index` | DM (simple-margin shortcut) | Current index rate, decimal. |
| `govt_curve` | G-spread | **Required.** Separate government curve handle. |

## C example

```c
#include "convex.h"   // generated by cbindgen
#include <stdio.h>

int main(void) {
    const char* bond_spec =
        "{\"type\":\"fixed_rate\",\"cusip\":\"TEST10Y5\","
        "\"coupon_rate\":0.05,\"frequency\":\"SemiAnnual\","
        "\"maturity\":\"2035-01-15\",\"issue\":\"2025-01-15\","
        "\"day_count\":\"Thirty360US\",\"currency\":\"USD\","
        "\"face_value\":100}";

    uint64_t bond = convex_bond_from_json(bond_spec);
    if (bond == 0) {
        fprintf(stderr, "build failed: %s\n", convex_last_error());
        return 1;
    }

    char request[512];
    snprintf(request, sizeof request,
        "{\"bond\":%llu,\"settlement\":\"2025-04-15\","
        "\"mark\":\"99.5C\",\"quote_frequency\":\"SemiAnnual\"}",
        (unsigned long long)bond);

    const char* response = convex_price(request);
    printf("%s\n", response);
    convex_string_free((char*)response);

    convex_release(bond);
    return 0;
}
```

## Python (ctypes)

```python
import ctypes, json

lib = ctypes.CDLL("./libconvex_ffi.so")
lib.convex_bond_from_json.argtypes = [ctypes.c_char_p]
lib.convex_bond_from_json.restype  = ctypes.c_uint64
# IMPORTANT: functions that return Rust-owned strings must use c_void_p, not
# c_char_p. ctypes auto-copies c_char_p into a Python bytes and discards the
# original pointer, leaving nothing to pass back to convex_string_free.
lib.convex_price.argtypes          = [ctypes.c_char_p]
lib.convex_price.restype           = ctypes.c_void_p
lib.convex_string_free.argtypes    = [ctypes.c_void_p]

bond = lib.convex_bond_from_json(json.dumps({
    "type": "fixed_rate", "cusip": "TEST10Y5",
    "coupon_rate": 0.05, "frequency": "SemiAnnual",
    "maturity": "2035-01-15", "issue": "2025-01-15",
    "day_count": "Thirty360US", "currency": "USD", "face_value": 100,
}).encode())

ptr = lib.convex_price(json.dumps({
    "bond": bond, "settlement": "2025-04-15",
    "mark": "99.5C", "quote_frequency": "SemiAnnual",
}).encode())
try:
    response = ctypes.string_at(ptr).decode()
    print(json.loads(response))
finally:
    lib.convex_string_free(ptr)

lib.convex_release(bond)
```

## Thread safety

The registry uses `RwLock<HashMap<...>>` for concurrent reads. Every public
FFI function is safe to call from multiple threads. The thread-local
`last_error` slot is per-thread; check it on the same thread that triggered
the failure.

## Memory model

- Strings returned by `convex_*` (any function with `*const c_char` or
  `*mut c_char` return type **other than** `convex_last_error` and
  `convex_version`) are heap-allocated by Rust; free with `convex_string_free`.
- `convex_last_error` returns a pointer into a thread-local `CString`; it
  remains valid until the next call into the library on the same thread.
- `convex_version` returns a `'static` pointer.
- Inputs are borrowed; the FFI never takes ownership of caller-allocated
  memory.

## Tests

```bash
cargo test -p convex-ffi --test smoke --release -- --test-threads=1
```

19 end-to-end tests: every spread family (Z / G / I / ASW / OAS / DM),
FRN / zero / sinking-fund pricing and risk, KRD, cashflow tag stability,
schema introspection, mark parsing, error envelopes.

## License

MIT — see [LICENSE](../../LICENSE).
