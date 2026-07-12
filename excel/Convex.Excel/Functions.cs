using System;
using System.Globalization;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using Convex.Excel.Helpers;
using ExcelDna.Integration;

namespace Convex.Excel
{
    // The entire user-facing UDF surface.
    //
    // Stateful (handles): CX.BOND, CX.BOND.CALLABLE, CX.BOND.FRN, CX.BOND.ZERO,
    //                     CX.CURVE, CX.CURVE.BOOTSTRAP, CX.RELEASE, CX.OBJECTS, CX.CLEAR.
    // Stateless:          CX.PRICE, CX.RISK, CX.SPREAD, CX.CASHFLOWS, CX.CURVE.QUERY.
    // Diagnostic:         CX.SCHEMA, CX.MARK, CX.VERSION.
    //
    // Adding a new bond shape, spread family, or pricing convention does not
    // touch this file. The Rust DTO enum picks it up; the existing UDFs route
    // it.
    //
    // Threading: the stateless analytics (PRICE/RISK/SPREAD/CASHFLOWS/MW/
    // CURVE.QUERY) are IsThreadSafe so Excel's multi-threaded recalc runs them in
    // parallel — the native registry guards reads with a lock and clones objects
    // out before computing. Builders mutate the registry and use xlfCaller, so
    // they stay on the main calc thread.
    public static class Functions
    {
        // ===================================================================
        // Bond construction
        // ===================================================================

        [ExcelFunction(Name = "CX.BOND",
            Description = "Creates a fixed-rate bond (coupon as decimal, e.g. 0.05).",
            Category = "Convex Bonds")]
        public static object CxBond(
            [ExcelArgument("Optional CUSIP/ISIN/name (used as registry key)")] object id,
            [ExcelArgument("Coupon rate as decimal (0.05 for 5%)")] double couponDecimal,
            [ExcelArgument("Maturity date")] DateTime maturity,
            [ExcelArgument("Issue date")] DateTime issue,
            [ExcelArgument("Frequency: A | SA (default) | Q | M, or 1/2/4/12")] object frequency,
            [ExcelArgument("Day count (default Thirty360US)")] object dayCount,
            [ExcelArgument("Currency (default USD)")] object currency,
            [ExcelArgument("Face value (default 100)")] object faceValue) =>
            Safe(() => CxParse.FormatHandle(Cx.BuildBond(BondSpecs.FixedRate(
                AsString(id, ""), couponDecimal,
                CxParse.AsFrequency(frequency),
                maturity, issue,
                CxParse.AsDayCount(dayCount),
                AsString(currency, "USD"),
                AsDouble(faceValue, 100.0)))));

        [ExcelFunction(Name = "CX.BOND.CALLABLE",
            Description = "Creates a callable bond. Pass call dates and prices as parallel ranges.",
            Category = "Convex Bonds")]
        public static object CxBondCallable(
            object id,
            [ExcelArgument("Coupon rate as decimal")] double couponDecimal,
            DateTime maturity,
            DateTime issue,
            [ExcelArgument("Call dates (range)")] object callDates,
            [ExcelArgument("Call prices % of par, parallel to dates")] object callPrices,
            [ExcelArgument("Frequency, default SA")] object frequency,
            [ExcelArgument("Style: american (default) | european | bermudan | make_whole")] object callStyle,
            [ExcelArgument("Day count, default Thirty360US")] object dayCount,
            [ExcelArgument("Make-whole spread, basis points (only for make_whole)")] object makeWholeSpreadBps) =>
            Safe(() =>
            {
                var dates = CxParse.AsDatesStrict(callDates, "call dates");
                var prices = CxParse.AsDoublesStrict(callPrices, "call prices");
                if (dates.Length == 0)
                    throw new ConvexException("call dates range is empty");
                if (dates.Length != prices.Length)
                    throw new ConvexException(
                        $"call dates ({dates.Length}) and call prices ({prices.Length}) must be the same length");
                var schedule = new JArray();
                for (int i = 0; i < dates.Length; i++)
                    schedule.Add(new JObject
                    {
                        ["date"] = CxParse.AsIsoDate(dates[i]),
                        ["price"] = prices[i],
                    });
                double? mwSpread = IsBlank(makeWholeSpreadBps) ? (double?)null : AsDouble(makeWholeSpreadBps, 0.0);
                return CxParse.FormatHandle(Cx.BuildBond(BondSpecs.Callable(
                    AsString(id, ""), couponDecimal,
                    CxParse.AsFrequency(frequency),
                    maturity, issue,
                    schedule,
                    AsString(callStyle, "american").ToLowerInvariant(),
                    CxParse.AsDayCount(dayCount),
                    mwSpread)));
            });

        [ExcelFunction(Name = "CX.BOND.FRN",
            Description = "Creates a floating rate note.",
            Category = "Convex Bonds")]
        public static object CxBondFrn(
            object id,
            [ExcelArgument("Spread over the index in basis points")] double spreadBps,
            DateTime maturity,
            DateTime issue,
            [ExcelArgument("Index: SOFR (default), SONIA, ESTR, TONAR, SARON, CORRA, EURIBOR3M, EURIBOR6M, TIBOR3M")] object rateIndex,
            [ExcelArgument("Frequency, default Q")] object frequency,
            [ExcelArgument("Day count, default Act360")] object dayCount,
            [ExcelArgument("Cap (decimal, optional)")] object cap,
            [ExcelArgument("Floor (decimal, optional)")] object floor) =>
            Safe(() => CxParse.FormatHandle(Cx.BuildBond(BondSpecs.Frn(
                AsString(id, ""), spreadBps, maturity, issue,
                AsString(rateIndex, "sofr").ToLowerInvariant(),
                CxParse.AsFrequency(frequency, "Quarterly"),
                CxParse.AsDayCount(dayCount, "Act360"),
                IsBlank(cap) ? (double?)null : AsDouble(cap, 0),
                IsBlank(floor) ? (double?)null : AsDouble(floor, 0)))));

        [ExcelFunction(Name = "CX.BOND.ZERO",
            Description = "Creates a zero-coupon bond.",
            Category = "Convex Bonds")]
        public static object CxBondZero(
            object id,
            DateTime maturity,
            DateTime issue,
            [ExcelArgument("Compounding (default SemiAnnual)")] object compounding,
            [ExcelArgument("Day count (default ActActIcma)")] object dayCount) =>
            Safe(() => CxParse.FormatHandle(Cx.BuildBond(BondSpecs.ZeroCoupon(
                AsString(id, ""), maturity, issue,
                AsString(compounding, "SemiAnnual"),
                CxParse.AsDayCount(dayCount, "ActActIcma")))));

        // ===================================================================
        // Curve construction
        // ===================================================================

        [ExcelFunction(Name = "CX.CURVE",
            Description = "Creates a discrete curve from tenor/value points.",
            Category = "Convex Curves")]
        public static object CxCurve(
            [ExcelArgument("Optional curve name")] object name,
            DateTime refDate,
            [ExcelArgument("Tenors in years (range)")] object tenors,
            [ExcelArgument("Values: zero rates as decimal (default), or DFs if value_kind=df")] object values,
            [ExcelArgument("Value kind: zero_rate (default) | discount_factor")] object valueKind,
            [ExcelArgument("Interpolation: linear (default) | log_linear | cubic_spline | monotone_convex")] object interpolation,
            [ExcelArgument("Day count (default Act365Fixed)")] object dayCount,
            [ExcelArgument("Compounding (default Continuous)")] object compounding) =>
            Safe(() =>
            {
                var t = CxParse.AsDoublesStrict(tenors, "tenors");
                var v = CxParse.AsDoublesStrict(values, "values");
                if (t.Length == 0)
                    throw new ConvexException("tenors range is empty");
                if (t.Length != v.Length)
                    throw new ConvexException(
                        $"tenors ({t.Length}) and values ({v.Length}) must be the same length");
                return CxParse.FormatHandle(Cx.BuildCurve(CurveSpecs.Discrete(
                    AsString(name, ""), refDate,
                    ToJsonArray(t), ToJsonArray(v),
                    AsString(valueKind, "zero_rate").ToLowerInvariant(),
                    AsString(interpolation, "linear").ToLowerInvariant(),
                    CxParse.AsDayCount(dayCount, "Act365Fixed"),
                    AsString(compounding, "Continuous"))));
            });

        [ExcelFunction(Name = "CX.CURVE.BOOTSTRAP",
            Description = "Bootstraps a curve from market instruments.",
            Category = "Convex Curves")]
        public static object CxCurveBootstrap(
            object name,
            DateTime refDate,
            [ExcelArgument("Instrument kinds: deposit | fra | swap | ois (range)")] object kinds,
            [ExcelArgument("Tenors in years (parallel range)")] object tenors,
            [ExcelArgument("Rates as decimals (parallel range)")] object rates,
            [ExcelArgument("Method: global_fit (default) | piecewise")] object method,
            [ExcelArgument("Interpolation, default linear")] object interpolation,
            [ExcelArgument("Day count, default Act360")] object dayCount) =>
            Safe(() =>
            {
                var ks = CxParse.AsStringsStrict(kinds, "kinds");
                var ts = CxParse.AsDoublesStrict(tenors, "tenors");
                var rs = CxParse.AsDoublesStrict(rates, "rates");
                if (ks.Length == 0)
                    throw new ConvexException("kinds range is empty");
                if (ks.Length != ts.Length || ts.Length != rs.Length)
                    throw new ConvexException(
                        $"kinds ({ks.Length}), tenors ({ts.Length}) and rates ({rs.Length}) must be the same length");
                var insts = new JArray();
                for (int i = 0; i < ks.Length; i++)
                    insts.Add(new JObject
                    {
                        ["kind"] = ks[i].ToLowerInvariant(),
                        ["tenor"] = ts[i],
                        ["rate"] = rs[i],
                    });
                return CxParse.FormatHandle(Cx.BuildCurve(CurveSpecs.Bootstrap(
                    AsString(name, ""), refDate,
                    AsString(method, "global_fit").ToLowerInvariant(),
                    insts,
                    AsString(interpolation, "linear").ToLowerInvariant(),
                    CxParse.AsDayCount(dayCount, "Act360"))));
            });

        // ===================================================================
        // Stateless analytics RPCs
        // ===================================================================

        [ExcelFunction(Name = "CX.PRICE",
            Description = "Prices a bond against a trader mark and returns clean/dirty/accrued/ytm.",
            Category = "Convex Pricing", IsThreadSafe = true)]
        public static object CxPrice(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Mark: 99.5C, 99.5D, 4.65%, 4.65%@SA, +125bps@USD.SOFR, 99-16+")] object mark,
            [ExcelArgument("Curve handle or name (only required for spread marks)")] object curveRef,
            [ExcelArgument("Quote frequency for derived YTM, default SA")] object quoteFrequency,
            [ExcelArgument("Field: clean (default) | dirty | accrued | ytm | z_spread | grid")] object field) =>
            Safe(() =>
            {
                var req = BuildPriceRequest(bondRef, settlement, mark, curveRef, quoteFrequency);
                var result = Cx.Price(req);
                return SelectPriceField(result, AsString(field, "clean").ToLowerInvariant());
            });

        [ExcelFunction(Name = "CX.PRICE.LIVE",
            Description = "Live CX.PRICE: the cell updates automatically whenever any referenced " +
                          "bond or curve is rebuilt (RTD subscription, like BDP).",
            Category = "Convex Pricing")]
        public static object CxPriceLive(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Curve handle or name (only required for spread marks)")] object curveRef,
            [ExcelArgument("Quote frequency for derived YTM, default SA")] object quoteFrequency,
            [ExcelArgument("Field: clean (default) | dirty | accrued | ytm | z_spread | grid")] object field) =>
            Safe(() =>
            {
                var req = BuildPriceRequest(bondRef, settlement, mark, curveRef, quoteFrequency);
                var f = AsString(field, "clean").ToLowerInvariant();
                return CxLive.Observe("price", req, env => SelectPriceField(env, f));
            });

        private static JObject BuildPriceRequest(
            object bondRef, DateTime settlement, object mark, object curveRef, object quoteFrequency)
        {
            var req = new JObject
            {
                ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                ["settlement"] = CxParse.AsIsoDate(settlement),
                ["mark"] = CxParse.AsMark(mark),
                ["quote_frequency"] = CxParse.AsFrequency(quoteFrequency),
            };
            var curve = CxParse.AsHandleRefOrNull(curveRef);
            if (curve != null) req["curve"] = curve;
            return req;
        }

        [ExcelFunction(Name = "CX.RISK",
            Description = "Returns risk metrics. Default returns a 2D grid; pass a metric name for a scalar.",
            Category = "Convex Risk", IsThreadSafe = true)]
        public static object CxRisk(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Curve handle or name (spread marks + KRD only)")] object curveRef,
            [ExcelArgument("Metric: grid (default) | mod_dur | mac_dur | convexity | dv01 | spread_dur | krd")] object metric,
            [ExcelArgument("Quote frequency, default SA")] object quoteFrequency,
            [ExcelArgument("Key-rate tenors (years) for KRD; range or csv string")] object keyRateTenors) =>
            Safe(() =>
            {
                var req = BuildRiskRequest(bondRef, settlement, mark, curveRef, quoteFrequency, keyRateTenors);
                var result = Cx.Risk(req);
                return SelectRiskField(result, AsString(metric, "grid").ToLowerInvariant());
            });

        [ExcelFunction(Name = "CX.RISK.LIVE",
            Description = "Live CX.RISK: updates automatically when referenced objects change (RTD).",
            Category = "Convex Risk")]
        public static object CxRiskLive(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Curve handle or name (spread marks + KRD only)")] object curveRef,
            [ExcelArgument("Metric: grid (default) | mod_dur | mac_dur | convexity | dv01 | spread_dur | krd")] object metric,
            [ExcelArgument("Quote frequency, default SA")] object quoteFrequency,
            [ExcelArgument("Key-rate tenors (years) for KRD; range or csv string")] object keyRateTenors) =>
            Safe(() =>
            {
                var req = BuildRiskRequest(bondRef, settlement, mark, curveRef, quoteFrequency, keyRateTenors);
                var m = AsString(metric, "grid").ToLowerInvariant();
                return CxLive.Observe("risk", req, env => SelectRiskField(env, m));
            });

        private static JObject BuildRiskRequest(
            object bondRef, DateTime settlement, object mark, object curveRef,
            object quoteFrequency, object keyRateTenors)
        {
            var req = new JObject
            {
                ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                ["settlement"] = CxParse.AsIsoDate(settlement),
                ["mark"] = CxParse.AsMark(mark),
                ["quote_frequency"] = CxParse.AsFrequency(quoteFrequency),
            };
            var curve = CxParse.AsHandleRefOrNull(curveRef);
            if (curve != null) req["curve"] = curve;

            var tenors = ParseTenors(keyRateTenors);
            if (tenors.Length > 0)
            {
                var arr = new JArray();
                foreach (var t in tenors) arr.Add(t);
                req["key_rate_tenors"] = arr;
            }
            return req;
        }

        [ExcelFunction(Name = "CX.SPREAD",
            Description = "Computes a spread (Z, G, I, ASW, OAS, DM, …) at the given mark.",
            Category = "Convex Spreads", IsThreadSafe = true)]
        public static object CxSpread(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Curve handle or name")] object curveRef,
            DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Spread: Z (default) | G | I | OAS | DM | ASW | ASW_PROC | CREDIT")] object spreadType,
            [ExcelArgument("OAS volatility in percent (1 = 1% = 0.01 decimal vol); default 1")] object volatility,
            [ExcelArgument("Field: bps (default) | grid")] object field) =>
            Safe(() =>
            {
                var req = BuildSpreadRequest(bondRef, curveRef, settlement, mark, spreadType, volatility);
                var result = Cx.Spread(req);
                return SelectSpreadField(result, AsString(field, "bps").ToLowerInvariant());
            });

        [ExcelFunction(Name = "CX.SPREAD.LIVE",
            Description = "Live CX.SPREAD: updates automatically when referenced objects change (RTD).",
            Category = "Convex Spreads")]
        public static object CxSpreadLive(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Curve handle or name")] object curveRef,
            DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Spread: Z (default) | G | I | OAS | DM | ASW | ASW_PROC | CREDIT")] object spreadType,
            [ExcelArgument("OAS volatility in percent (1 = 1% = 0.01 decimal vol); default 1")] object volatility,
            [ExcelArgument("Field: bps (default) | grid")] object field) =>
            Safe(() =>
            {
                var req = BuildSpreadRequest(bondRef, curveRef, settlement, mark, spreadType, volatility);
                var f = AsString(field, "bps").ToLowerInvariant();
                return CxLive.Observe("spread", req, env => SelectSpreadField(env, f));
            });

        private static JObject BuildSpreadRequest(
            object bondRef, object curveRef, DateTime settlement, object mark,
            object spreadType, object volatility)
        {
            var req = new JObject
            {
                ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                ["curve"] = CxParse.AsHandleRef(curveRef, "curve"),
                ["settlement"] = CxParse.AsIsoDate(settlement),
                ["mark"] = CxParse.AsMark(mark),
                ["spread_type"] = CxParse.AsSpreadType(AsString(spreadType, "Z")),
            };
            if (!IsBlank(volatility))
                req["params"] = new JObject { ["volatility"] = AsDouble(volatility, 0.01) / 100.0 };
            return req;
        }

        [ExcelFunction(Name = "CX.CASHFLOWS",
            Description = "Bond cashflow schedule on or after settlement.",
            Category = "Convex Bonds", IsThreadSafe = true)]
        public static object CxCashflows(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            DateTime settlement) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                };
                var result = Cx.Cashflows(req);
                return CashflowsToGrid(result);
            });

        [ExcelFunction(Name = "CX.MW",
            Description = "Make-whole call price for a callable bond carrying a make-whole spread. " +
                          "Returns price (default), discount_rate, or spread_bps.",
            Category = "Convex Bonds", IsThreadSafe = true)]
        public static object CxMakeWhole(
            object bondRef,
            [ExcelArgument("Hypothetical call date")] DateTime callDate,
            [ExcelArgument("Treasury par yield, decimal (0.05 = 5%)")] double treasuryRate,
            [ExcelArgument("Field: price (default) | discount_rate | spread_bps")] object field) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                    ["call_date"] = CxParse.AsIsoDate(callDate),
                    ["treasury_rate"] = treasuryRate,
                };
                var result = Cx.MakeWhole(req);
                var f = AsString(field, "price").ToLowerInvariant();
                return f switch
                {
                    "price" => (double?)result["price"] ?? throw new ConvexException("missing price"),
                    "discount_rate" => (double?)result["discount_rate"] ?? throw new ConvexException("missing discount_rate"),
                    "spread_bps" => (double?)result["spread_bps"] ?? throw new ConvexException("missing spread_bps"),
                    _ => throw new ConvexException($"unknown MW field: {f}", ErrorCodes.UnknownToken),
                };
            });

        [ExcelFunction(Name = "CX.YAS",
            Description = "One-call yield & spread analysis (Bloomberg-YAS style): every yield convention, " +
                          "G/Z/benchmark/ASW spreads, risk metrics, and the settlement invoice as one spilled grid.",
            Category = "Convex Pricing", IsThreadSafe = true)]
        public static object CxYas(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Spot/discount curve handle or name")] object curveRef,
            [ExcelArgument("Government curve for G-spread (optional; defaults to the spot curve)")] object govtCurveRef,
            [ExcelArgument("Quote frequency, default SA")] object quoteFrequency,
            [ExcelArgument("Swap curve for the asset-swap spread (optional)")] object swapCurveRef) =>
            Safe(() =>
            {
                var req = BuildYasRequest(bondRef, settlement, mark, curveRef, govtCurveRef,
                    quoteFrequency, swapCurveRef);
                return YasToGrid(Cx.Yas(req));
            });

        [ExcelFunction(Name = "CX.YAS.LIVE",
            Description = "Live CX.YAS: the analysis grid refreshes automatically when referenced " +
                          "objects change (RTD).",
            Category = "Convex Pricing")]
        public static object CxYasLive(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Spot/discount curve handle or name")] object curveRef,
            [ExcelArgument("Government curve for G-spread (optional; defaults to the spot curve)")] object govtCurveRef,
            [ExcelArgument("Quote frequency, default SA")] object quoteFrequency,
            [ExcelArgument("Swap curve for the asset-swap spread (optional)")] object swapCurveRef) =>
            Safe(() =>
            {
                var req = BuildYasRequest(bondRef, settlement, mark, curveRef, govtCurveRef,
                    quoteFrequency, swapCurveRef);
                return CxLive.Observe("yas", req, YasToGrid);
            });

        private static JObject BuildYasRequest(
            object bondRef, DateTime settlement, object mark, object curveRef,
            object govtCurveRef, object quoteFrequency, object swapCurveRef)
        {
            var req = new JObject
            {
                ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                ["settlement"] = CxParse.AsIsoDate(settlement),
                ["mark"] = CxParse.AsMark(mark),
                ["curve"] = CxParse.AsHandleRef(curveRef, "curve"),
                ["quote_frequency"] = CxParse.AsFrequency(quoteFrequency),
            };
            var govt = CxParse.AsHandleRefOrNull(govtCurveRef);
            if (govt != null) req["govt_curve"] = govt;
            var swap = CxParse.AsHandleRefOrNull(swapCurveRef);
            if (swap != null) req["swap_curve"] = swap;
            return req;
        }

        private static object YasToGrid(JToken r)
        {
                var rows = new System.Collections.Generic.List<(string, object)>
                {
                    ("Clean", (double?)r["clean_price"] ?? double.NaN),
                    ("Dirty", (double?)r["dirty_price"] ?? double.NaN),
                    ("Accrued", (double?)r["accrued"] ?? double.NaN),
                    ("Accrued Days", (double?)r["accrued_days"] ?? double.NaN),
                    ("YTM (%)", (double?)r["ytm_pct"] ?? double.NaN),
                    ("Current Yield (%)", (double?)r["current_yield_pct"] ?? double.NaN),
                    ("Simple Yield (%)", (double?)r["simple_yield_pct"] ?? double.NaN),
                };
                var mmy = (double?)r["money_market_yield_pct"];
                if (mmy.HasValue) rows.Add(("MM Yield (%)", mmy.Value));
                rows.Add(("G-Spread (bps)", (double?)r["g_spread_bps"] ?? double.NaN));
                rows.Add(("Z-Spread (bps)", (double?)r["z_spread_bps"] ?? double.NaN));
                rows.Add(($"Benchmark {(string?)r["benchmark_tenor"] ?? "?"} (bps)",
                    (double?)r["benchmark_spread_bps"] ?? double.NaN));
                var asw = (double?)r["asw_spread_bps"];
                if (asw.HasValue) rows.Add(("ASW (bps)", asw.Value));
                var oas = (double?)r["oas_bps"];
                if (oas.HasValue) rows.Add(("OAS (bps)", oas.Value));
                rows.Add(("Modified Duration", (double?)r["modified_duration"] ?? double.NaN));
                rows.Add(("Macaulay Duration", (double?)r["macaulay_duration"] ?? double.NaN));
                rows.Add(("Convexity", (double?)r["convexity"] ?? double.NaN));
                rows.Add(("DV01", (double?)r["dv01_per_100"] ?? double.NaN));
                rows.Add(("Principal", (double?)r["principal_amount"] ?? double.NaN));
                rows.Add(("Accrued Amt", (double?)r["accrued_amount"] ?? double.NaN));
                rows.Add(("Settlement Total", (double?)r["settlement_amount"] ?? double.NaN));

                var g = new object[rows.Count, 2];
                for (int i = 0; i < rows.Count; i++) { g[i, 0] = rows[i].Item1; g[i, 1] = rows[i].Item2; }
                return g;
        }

        [ExcelFunction(Name = "CX.CURVE.QUERY",
            Description = "Read a curve point: zero rate (default), discount factor, or forward rate.",
            Category = "Convex Curves", IsThreadSafe = true)]
        public static object CxCurveQuery(
            [ExcelArgument("Curve handle or name")] object curveRef,
            [ExcelArgument("Tenor in years")] double tenor,
            [ExcelArgument("Query: zero (default) | df | forward")] object query,
            [ExcelArgument("End tenor (forward only)")] object tenorEnd) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["curve"] = CxParse.AsHandleRef(curveRef, "curve"),
                    ["query"] = AsString(query, "zero").ToLowerInvariant(),
                    ["tenor"] = tenor,
                };
                if (!IsBlank(tenorEnd)) req["tenor_end"] = AsDouble(tenorEnd, tenor + 0.25);
                var result = Cx.CurveQuery(req);
                return (double?)result["value"] ?? throw new ConvexException("missing value");
            });

        [ExcelFunction(Name = "CX.SCENARIO",
            Description = "Curve scenario ladder in one call: reprices the bond under each shift " +
                          "holding the mark-implied Z-spread fixed. Spills one row per scenario.",
            Category = "Convex Risk", IsThreadSafe = true)]
        public static object CxScenario(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Curve handle or name")] object curveRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Base mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Shifts in bps (range or csv), one scenario per value")] object shifts,
            [ExcelArgument("Kind: parallel (default) | steepener | flattener | key_rate | credit")] object kind,
            [ExcelArgument("Pivot tenor (steepener/flattener) or key tenor (key_rate), years; default 5")] object pivotOrTenor,
            [ExcelArgument("Quote frequency, default SA")] object quoteFrequency) =>
            Safe(() =>
            {
                var shiftValues = ParseShifts(shifts, "shifts");
                if (shiftValues.Length == 0)
                    throw new ConvexException("shifts: at least one bps value is required");

                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                    ["curve"] = CxParse.AsHandleRef(curveRef, "curve"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                    ["mark"] = CxParse.AsMark(mark),
                    ["scenarios"] = BuildScenarioLadder(
                        shiftValues,
                        AsString(kind, "parallel").ToLowerInvariant(),
                        AsDouble(pivotOrTenor, 5.0)),
                    ["quote_frequency"] = CxParse.AsFrequency(quoteFrequency),
                };

                var result = Cx.Scenario(req);
                var rows = result["rows"] as JArray ?? new JArray();
                var g = new object[rows.Count + 1, 5];
                g[0, 0] = "Scenario"; g[0, 1] = "Clean"; g[0, 2] = "Dirty";
                g[0, 3] = "ΔClean"; g[0, 4] = "YTM (%)";
                for (int i = 0; i < rows.Count; i++)
                {
                    var row = rows[i];
                    g[i + 1, 0] = (string?)row["name"] ?? "";
                    g[i + 1, 1] = (double?)row["clean_price"] ?? double.NaN;
                    g[i + 1, 2] = (double?)row["dirty_price"] ?? double.NaN;
                    g[i + 1, 3] = (double?)row["delta_clean"] ?? double.NaN;
                    g[i + 1, 4] = ((double?)row["ytm_decimal"] ?? double.NaN) * 100.0;
                }
                return g;
            });

        // ===================================================================
        // Hedge advisor
        // ===================================================================

        [ExcelFunction(Name = "CX.RISKPROFILE",
            Description = "Builds a position risk profile (DV01, KRD buckets, market value) and returns " +
                          "its JSON — feed it to CX.HEDGE.",
            Category = "Convex Risk", IsThreadSafe = true)]
        public static object CxRiskProfile(
            [ExcelArgument("Bond handle, CUSIP, ISIN, or name")] object bondRef,
            [ExcelArgument("Discount curve handle or name")] object curveRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Position face amount, e.g. 10000000")] double notionalFace,
            [ExcelArgument("Short-rate vol, decimal (callables only)")] object volatility) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandleRef(bondRef, "bond"),
                    ["curve"] = CxParse.AsHandleRef(curveRef, "curve"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                    ["mark"] = CxParse.AsMark(mark),
                    ["notional_face"] = notionalFace,
                };
                if (!IsBlank(volatility)) req["volatility"] = AsDouble(volatility, 0.01);
                return Cx.RiskProfile(req).ToString(Formatting.None);
            });

        [ExcelFunction(Name = "CX.HEDGE",
            Description = "Proposes a hedge for a CX.RISKPROFILE position. Returns a grid of trades + " +
                          "residual DV01 + cost.",
            Category = "Convex Risk", IsThreadSafe = true)]
        public static object CxHedge(
            [ExcelArgument("Strategy: duration_futures | barbell_futures | cash_bond_pair | interest_rate_swap | key_rate_futures")] string strategy,
            [ExcelArgument("Position profile JSON from CX.RISKPROFILE")] string profileJson,
            [ExcelArgument("Discount curve handle or name")] object curveRef,
            [ExcelArgument("Settlement date")] DateTime settlement) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["strategy"] = strategy.Trim().ToLowerInvariant(),
                    ["position"] = JToken.Parse(profileJson),
                    ["curve"] = CxParse.AsHandleRef(curveRef, "curve"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                };
                var r = Cx.Hedge(req);
                var trades = r["trades"] as JArray ?? new JArray();
                var g = new object[trades.Count + 3, 3];
                g[0, 0] = "Trade"; g[0, 1] = "Quantity"; g[0, 2] = "DV01";
                for (int i = 0; i < trades.Count; i++)
                {
                    var t = trades[i];
                    g[i + 1, 0] = t?["instrument"]?.ToString(Formatting.None) ?? "?";
                    g[i + 1, 1] = (double?)t?["quantity"] ?? double.NaN;
                    g[i + 1, 2] = (double?)t?["dv01"] ?? double.NaN;
                }
                g[trades.Count + 1, 0] = "Residual DV01";
                g[trades.Count + 1, 1] = (double?)r["residual"]?["residual_dv01"] ?? double.NaN;
                g[trades.Count + 1, 2] = "";
                g[trades.Count + 2, 0] = "Cost (bps)";
                g[trades.Count + 2, 1] = (double?)r["cost_bps"] ?? double.NaN;
                g[trades.Count + 2, 2] = "";
                return g;
            });

        // ===================================================================
        // Diagnostics
        // ===================================================================

        [ExcelFunction(Name = "CX.SCHEMA", Description = "Returns the JSON schema of a wire-format type.",
            Category = "Convex Utilities")]
        public static object CxSchema([ExcelArgument("Mark, BondSpec, CurveSpec, PricingRequest, …")] string typeName) =>
            Safe(() => Cx.Schema(typeName));

        [ExcelFunction(Name = "CX.MARK", Description = "Parse a textual mark and return its canonical JSON.",
            Category = "Convex Utilities")]
        public static object CxMark(string text) =>
            Safe(() => Cx.ParseMark(text)?.ToString(Formatting.None) ?? "");

        [ExcelFunction(Name = "CX.VERSION", Description = "Library version.",
            Category = "Convex Utilities")]
        public static string CxVersion() { try { return Cx.Version(); } catch { return "ERROR"; } }

        [ExcelFunction(Name = "CX.OBJECTS", Description = "Number of registered objects.",
            Category = "Convex Utilities")]
        public static int CxObjects() { try { return Cx.ObjectCount(); } catch { return -1; } }

        [ExcelFunction(Name = "CX.RELEASE", Description = "Releases an object handle.",
            Category = "Convex Utilities")]
        public static object CxRelease(object handle) =>
            Safe(() => { Cx.Release(CxParse.AsHandle(handle, "handle")); return "OK"; });

        [ExcelFunction(Name = "CX.CLEAR", Description = "Releases all registered objects.",
            Category = "Convex Utilities")]
        public static object CxClear() =>
            Safe(() => { Cx.ClearAll(); CxErrorStore.Clear(); return "OK"; });

        [ExcelFunction(Name = "CX.DESCRIBE", Description = "JSON description of a registered object.",
            Category = "Convex Utilities")]
        public static object CxDescribe(object handle) =>
            Safe(() => Cx.Describe(CxParse.AsHandle(handle, "handle")));

        [ExcelFunction(Name = "CX.LASTERROR",
            Description = "Detail behind a CX.* error value. Point it at the failing cell: =CX.LASTERROR(B2).",
            Category = "Convex Utilities", IsVolatile = true, IsMacroType = true)]
        public static object CxLastError(
            [ExcelArgument(AllowReference = true, Description = "Reference to the failing cell")] object cellRef)
        {
            if (cellRef is ExcelReference r)
            {
                var d = CxErrorStore.Lookup(CxCaller.StableKey(r));
                return d == null
                    ? "(no error recorded for that cell)"
                    : $"[{d.Code}] {d.Message}";
            }
            return "(pass a cell reference, e.g. =CX.LASTERROR(B2))";
        }

        [ExcelFunction(Name = "CX.DIAG",
            Description = "Add-in diagnostics: native library status, version, object count.",
            Category = "Convex Utilities", IsVolatile = true)]
        public static object CxDiag()
        {
            string version;
            try { version = Cx.Version(); }
            catch (Exception ex) { version = "unavailable: " + ex.Message; }
            int objects;
            try { objects = Cx.ObjectCount(); }
            catch { objects = -1; }
            var grid = new object[4, 2];
            grid[0, 0] = "Native load";   grid[0, 1] = NativeLoader.GetLoadError();
            grid[1, 0] = "Engine";        grid[1, 1] = version;
            grid[2, 0] = "Objects";       grid[2, 1] = objects;
            grid[3, 0] = "Add-in";        grid[3, 1] = typeof(Functions).Assembly.GetName().Version?.ToString() ?? "?";
            return grid;
        }

        // ===================================================================
        // Helpers
        // ===================================================================

        // Failures surface as native Excel errors (#VALUE!, #REF!, #NUM!,
        // #NAME?, #N/A — see ErrorMapper) so IFERROR/ISERROR work and errors
        // propagate through dependents as errors, not as text. The full
        // message is recorded per cell; read it with =CX.LASTERROR(cell) or
        // the ribbon error log.
        private static object Safe(Func<object> body)
        {
            try { return body(); }
            catch (Exception ex)
            {
                CxErrorStore.Record(ex);
                return ErrorMapper.ToExcelError(ex);
            }
        }

        private static bool IsBlank(object value) => value is null or ExcelMissing or ExcelEmpty;

        private static string AsString(object value, string defaultValue)
        {
            if (IsBlank(value)) return defaultValue;
            return value is string s ? s : value!.ToString() ?? defaultValue;
        }

        private static double AsDouble(object value, double defaultValue)
        {
            if (IsBlank(value)) return defaultValue;
            return value switch
            {
                double d => d,
                string s when double.TryParse(s, NumberStyles.Any, CultureInfo.InvariantCulture, out var p) => p,
                _ => Convert.ToDouble(value, CultureInfo.InvariantCulture),
            };
        }

        private static JArray ToJsonArray(double[] values)
        {
            var arr = new JArray();
            foreach (var v in values) arr.Add(v);
            return arr;
        }

        private static object SelectPriceField(JToken result, string field)
        {
            switch (field)
            {
                case "clean": return (double?)result["clean_price"] ?? throw new ConvexException("clean missing");
                case "dirty": return (double?)result["dirty_price"] ?? throw new ConvexException("dirty missing");
                case "accrued": return (double?)result["accrued"] ?? 0.0;
                case "ytm": return ((double?)result["ytm_decimal"] ?? 0.0) * 100.0;
                case "z_spread":
                case "zspread":
                case "z":
                    {
                        var v = (double?)result["z_spread_bps"];
                        return v.HasValue ? (object)v.Value : ExcelError.ExcelErrorNA;
                    }
                case "grid":
                    var grid = new object[5, 2];
                    grid[0, 0] = "Clean";    grid[0, 1] = (double?)result["clean_price"] ?? double.NaN;
                    grid[1, 0] = "Dirty";    grid[1, 1] = (double?)result["dirty_price"] ?? double.NaN;
                    grid[2, 0] = "Accrued";  grid[2, 1] = (double?)result["accrued"] ?? double.NaN;
                    grid[3, 0] = "YTM (%)";  grid[3, 1] = ((double?)result["ytm_decimal"] ?? 0.0) * 100.0;
                    grid[4, 0] = "Z (bps)";  grid[4, 1] = (object?)((double?)result["z_spread_bps"]) ?? "n/a";
                    return grid;
                default: throw new ConvexException("unknown CX.PRICE field " + field, ErrorCodes.UnknownToken);
            }
        }

        private static double[] ParseTenors(object cell) => ParseShifts(cell, "key-rate tenors");

        // Range of numbers, or a csv/space-separated string of them.
        private static double[] ParseShifts(object cell, string fieldName)
        {
            if (IsBlank(cell)) return Array.Empty<double>();
            if (cell is string s)
            {
                return s.Split(new[] { ',', ';', ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries)
                    .Select(t => double.TryParse(t.Trim(), NumberStyles.Any, CultureInfo.InvariantCulture, out var v)
                        ? v
                        : throw new ConvexException($"{fieldName}: '{t}' is not a number"))
                    .ToArray();
            }
            return CxParse.AsDoublesStrict(cell, fieldName);
        }

        // One named single-bump scenario per shift, shared by =CX.SCENARIO and
        // the ribbon Scenario form so the two can't drift.
        internal static JArray BuildScenarioLadder(double[] shiftsBps, string kind, double pivotOrTenor)
        {
            var scenarios = new JArray();
            foreach (var s in shiftsBps)
            {
                JObject bump = kind switch
                {
                    "parallel" => new JObject { ["kind"] = "parallel", ["shift_bps"] = s },
                    "steepener" => new JObject
                    {
                        ["kind"] = "steepener",
                        ["short_shift_bps"] = s,
                        ["long_shift_bps"] = s,
                        ["pivot_tenor"] = pivotOrTenor,
                    },
                    "flattener" => new JObject
                    {
                        ["kind"] = "flattener",
                        ["short_shift_bps"] = s,
                        ["long_shift_bps"] = s,
                        ["pivot_tenor"] = pivotOrTenor,
                    },
                    "key_rate" or "krd" => new JObject
                    {
                        ["kind"] = "key_rate",
                        ["tenor"] = pivotOrTenor,
                        ["shift_bps"] = s,
                    },
                    "credit" or "credit_spread" => new JObject
                    {
                        ["kind"] = "credit_spread",
                        ["shift_bps"] = s,
                    },
                    _ => throw new ConvexException($"unknown scenario kind {kind}", ErrorCodes.UnknownToken),
                };
                scenarios.Add(new JObject
                {
                    ["name"] = $"{(s >= 0 ? "+" : "")}{s:0.#}bp {kind}",
                    ["bumps"] = new JArray { bump },
                });
            }
            return scenarios;
        }

        private static object SelectRiskField(JToken result, string metric)
        {
            switch (metric)
            {
                case "mod_dur":
                case "modified_duration": return (double?)result["modified_duration"] ?? throw new ConvexException("missing");
                case "mac_dur":
                case "macaulay_duration": return (double?)result["macaulay_duration"] ?? throw new ConvexException("missing");
                case "convexity": return (double?)result["convexity"] ?? throw new ConvexException("missing");
                case "dv01": return (double?)result["dv01"] ?? throw new ConvexException("missing");
                case "spread_dur":
                case "spread_duration":
                    {
                        var v = (double?)result["spread_duration"];
                        return v.HasValue ? (object)v.Value : ExcelError.ExcelErrorNA;
                    }
                case "krd":
                case "key_rates":
                    {
                        var arr = result["key_rates"] as JArray ?? new JArray();
                        if (arr.Count == 0) return ExcelError.ExcelErrorNA;
                        var g = new object[arr.Count + 1, 2];
                        g[0, 0] = "Tenor";
                        g[0, 1] = "KRD";
                        for (int i = 0; i < arr.Count; i++)
                        {
                            var item = arr[i] as JObject;
                            g[i + 1, 0] = (double?)item?["tenor"] ?? double.NaN;
                            g[i + 1, 1] = (double?)item?["duration"] ?? double.NaN;
                        }
                        return g;
                    }
                case "grid":
                    var grid = new object[4, 2];
                    grid[0, 0] = "Modified Duration"; grid[0, 1] = (double?)result["modified_duration"] ?? double.NaN;
                    grid[1, 0] = "Macaulay Duration"; grid[1, 1] = (double?)result["macaulay_duration"] ?? double.NaN;
                    grid[2, 0] = "Convexity"; grid[2, 1] = (double?)result["convexity"] ?? double.NaN;
                    grid[3, 0] = "DV01"; grid[3, 1] = (double?)result["dv01"] ?? double.NaN;
                    return grid;
                default: throw new ConvexException("unknown CX.RISK metric " + metric, ErrorCodes.UnknownToken);
            }
        }

        private static object SelectSpreadField(JToken result, string field)
        {
            switch (field)
            {
                case "bps": return (double?)result["spread_bps"] ?? throw new ConvexException("missing spread_bps");
                case "grid":
                    var rows = new System.Collections.Generic.List<(string, object)>
                    {
                        ("Spread (bps)", (double?)result["spread_bps"] ?? double.NaN),
                    };
                    void AddIf(string label, string key)
                    {
                        var v = (double?)result[key];
                        if (v.HasValue) rows.Add((label, v.Value));
                    }
                    AddIf("Spread DV01", "spread_dv01");
                    AddIf("Spread Duration", "spread_duration");
                    AddIf("Option Value", "option_value");
                    AddIf("Effective Duration", "effective_duration");
                    AddIf("Effective Convexity", "effective_convexity");
                    var g = new object[rows.Count, 2];
                    for (int i = 0; i < rows.Count; i++) { g[i, 0] = rows[i].Item1; g[i, 1] = rows[i].Item2; }
                    return g;
                default: throw new ConvexException("unknown CX.SPREAD field " + field, ErrorCodes.UnknownToken);
            }
        }

        private static object CashflowsToGrid(JToken result)
        {
            var arr = result["flows"] as JArray ?? new JArray();
            var g = new object[arr.Count + 1, 3];
            g[0, 0] = "Date"; g[0, 1] = "Amount"; g[0, 2] = "Kind";
            for (int i = 0; i < arr.Count; i++)
            {
                var item = arr[i] as JObject;
                var dateStr = (string?)item?["date"];
                // Hand Excel a real DateTime so cells sort/format as dates.
                // Fall back to "" when missing or unparsable.
                if (!string.IsNullOrEmpty(dateStr) &&
                    DateTime.TryParse(dateStr, CultureInfo.InvariantCulture,
                        DateTimeStyles.AssumeLocal, out var dt))
                {
                    g[i + 1, 0] = dt;
                }
                else
                {
                    g[i + 1, 0] = "";
                }
                g[i + 1, 1] = (double?)item?["amount"] ?? 0.0;
                g[i + 1, 2] = (string?)item?["kind"] ?? "";
            }
            return g;
        }
    }
}
