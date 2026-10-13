# Treasury bill price, yield and quote risk

Use `convex::treasury_bill::quote_treasury_bill` with complete intrinsic terms,
an active settlement date and exactly one `TreasuryBillQuote`: absolute original
face cash, annual decimal bank discount, or annual decimal investment rate.
`0.05` means 5 percent. The shared `zero` operation exposes the same calculation:

```json
{
  "kind": "treasury_bill",
  "settlement": "2026-10-13",
  "quote": {"kind": "investment", "decimal_rate": "0.05"}
}
```

Put this action inside the existing complete `ZeroInput` request. Supply original
issue/maturity, currency, face/redemption, identifiers, issue price, day count,
compounding, settlement lag and calendar revision. The response retains those
terms and the original quote; the named Treasury convention determines this
calculation independently of the retained generic zero quotation settings. The
calendar must resolve, but an explicit quote settlement is not silently advanced
by its business-day lag. All cash and rates use exact decimal strings.

Let `r` be actual remaining days, `y` the days from original issue to the same
date one year later, `F` face, `P` unrounded absolute purchase cash and `i` the
investment rate. `y` remains fixed at 365 or366 when settlement changes. Through
half a year, `P=F/(1+r*i/y)`. Beyond half a year,
`P=F/[(1+(r/y-1/2)*i)*(1+i/2)]`. Bank discount is
`d=(1-P/F)*360/r`. These implement [Treasury Appendix B VI](https://www.treasurydirect.gov/files/laws-and-regulations/auction-regulations-uoc/31-cfr-part-356.pdf)
and the [2022 issue-year clarification](https://www.treasurydirect.gov/files/laws-and-regulations/uoc-noncomp-increase.pdf).

The inverse uses the admissible rationalized quadratic root, checked Decimal
arithmetic and at most96 square-root refinements. It reprices within1e-18 per100;
failure returns no accepted result. Both long-bill discount factors must be
positive. Premium prices and negative rates within that domain are supported.
USD face redemption, a term within the one-year issue interval, face1e-15..1e15
and purchase-price/face1e-6..1e6 are required. February29 issue dates return explicit
Unsupported because the cited same-date anniversary needs an additional declared
rule. Other currencies or redemption economics require their own convention.

Results include unrounded absolute/per100 cash, both decimal rates, basis and
remaining days, the branch and actual inverse residual. Auction per100 price
rounds separately to six decimals with midpoint away from zero. Each named rate
axis has first/second price derivatives and a signed tangent change per100 for
a +1bp quote move. These are unrounded quotation sensitivities; callers must
apply actual signed face quantity. Curve-node risk and executed hedge economics
need their existing separate owners. Rounded auction price is never fed back
into inversion or risk.

The existing `bill_conversions` operation and old native helpers retain their
original limits and units. Use this explicit route for the supplied Treasury
convention. Independent Decimal60 controls, actual short/long/leap/seasoned/par/
premium cases, both inverses, h/h2 sensitivities and typed failures can be run:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 tests/scripts/treasury_bill_reference.py check
cargo run --locked -p convex --example treasury_bill
PYTHONDONTWRITEBYTECODE=1 python3 tests/scripts/treasury_bill_check.py \
  --binary target/debug/examples/treasury_bill
```

These are scoped engineering controls. Full convention packs, independent
CVX-064/065 approval, all mandatory library scope and host/platform qualification
remain separate. Windows/Excel has not been executed on this machine.
