# Milestone 4 — callable bonds

**Result: 97 / 97 pass, zero delta.**

Added callable-bond coverage without needing a tree model or OAS machinery.
Both the Ford Credit make-whole + par-call structure and the synthetic HY
multi-date step-down are now reconciled.

## Approach — YTC / YTW as deterministic

For a callable bond priced at clean price P:

* **YTC at call date D** with call price K is just the YTM of a *hypothetical
  bullet bond* with the same coupons up to D and a final redemption of K on D.
* **YTW** is the min of YTC across every future call date and YTM at final
  maturity.

Both sides express "bullet with non-par redemption" directly:

| | Convex | QuantLib |
|---|---|---|
| Coupons | `coupon_rate × 100` per period | `[coupon_rate]` × face=100 |
| Final redemption | `.redemption_value(call_price)` on the builder | `redemption=call_price` arg to `FixedRateBond` |

No tree, no Hull-White, no OAS. The existing vanilla-bond pricing path does the
work — which is what the earlier milestones already reconciled bit-for-bit.

## What's now reconciled

| Bond | Extra metrics | Pass |
|---|---|:---:|
| `F_6_798_2028` | YTC at 2028-10-07 (par-call), YTM, YTW, workout date | 11/11 ✓ |
| `SYNTH_HY_STEPDOWN_01` | YTC at 2026/2027/2028/2029 call dates, YTM, YTW, workout date | 14/14 ✓ |

Plus the 8 vanilla bonds from M3 continue to pass at 8/8 each, all of them
matching with 0 delta (every metric, every digit of the 10-decimal output).

## Numerical sanity check

* **Ford Credit** at UST 3Y reference yield (3.51%): 6.798% coupon trades at
  108.82 clean. YTC at par-call = 3.42%, YTW = 3.42% on 2028-10-07 (par-call
  wins because issuer has every incentive to redeem at par when the bond is
  trading 8 points above). Matches QL exactly.

* **SYNTH_HY_STEPDOWN_01** at UST 4Y reference yield (3.61%): 7.5% coupon
  trades at 115.31. YTC on each successive call date:
  * 2026-04-15 @ 103.750: **−26.99%** (4 months to call; huge negative yield
    because redemption is 11 points below market)
  * 2027-04-15 @ 101.875: −2.62%
  * 2028-04-15 @ 100.938: +1.11%
  * 2029-04-15 @ 100.000: +2.61%
  * Maturity at 100: +3.61%
  YTW = −26.99% (earliest call wins). Both Convex and QL handle the
  negative-yield regime cleanly and produce the same answer.

The step-down case is a real stress test — solver robustness across negative
yields, high-coupon/low-market-yield regime, and four workout dates per bond.
All passed.

## Still deferred (Milestone 5)

* **UST_FRN_2Y** — SOFR-indexed coupons, discount margin. Needs the historical
  SOFR fixing series (already pulled: `sofr_fixings.csv`, 499 rows) and a
  forward-rate projection model on both sides.
* **UST_TIPS_10Y** — real coupon + CPI-U index ratio. The real-yield reconciliation
  is straightforward (same math as a vanilla bullet); the inflation adjustment
  needs the index ratio pulled at valuation date.
* **UK / EU / JP discount curves** — BoE, ECB, MoF Japan endpoints documented
  in `curves.json`. Current placeholders use coupon-rate as reference yield,
  which keeps cross-library consistency but isn't market-realistic.
