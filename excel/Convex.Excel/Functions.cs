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
                var dates = ExtractDates(callDates);
                var prices = CxParse.AsDoubles(callPrices);
                if (dates.Length == 0 || dates.Length != prices.Length)
                    throw new ConvexException("call dates and prices must be parallel non-empty ranges");
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
                var t = CxParse.AsDoubles(tenors);
                var v = CxParse.AsDoubles(values);
                if (t.Length == 0 || t.Length != v.Length)
                    throw new ConvexException("tenors and values must be parallel non-empty ranges");
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
                var ks = ExtractStrings(kinds);
                var ts = CxParse.AsDoubles(tenors);
                var rs = CxParse.AsDoubles(rates);
                if (ks.Length == 0 || ks.Length != ts.Length || ts.Length != rs.Length)
                    throw new ConvexException("kinds, tenors, and rates must be parallel non-empty ranges");
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
            Category = "Convex Pricing")]
        public static object CxPrice(
            [ExcelArgument("Bond handle")] object bondRef,
            [ExcelArgument("Settlement date")] DateTime settlement,
            [ExcelArgument("Mark: 99.5C, 99.5D, 4.65%, 4.65%@SA, +125bps@USD.SOFR, 99-16+")] object mark,
            [ExcelArgument("Curve handle (only required for spread marks)")] object curveRef,
            [ExcelArgument("Quote frequency for derived YTM, default SA")] object quoteFrequency,
            [ExcelArgument("Field: clean (default) | dirty | accrued | ytm | z_spread | grid")] object field) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandle(bondRef, "bond"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                    ["mark"] = CxParse.AsMark(mark),
                    ["quote_frequency"] = CxParse.AsFrequency(quoteFrequency),
                };
                var curveHandle = CxParse.AsHandleOrNull(curveRef);
                if (curveHandle is ulong c) req["curve"] = c;

                var result = Cx.Price(req);
                return SelectPriceField(result, AsString(field, "clean").ToLowerInvariant());
            });

        [ExcelFunction(Name = "CX.RISK",
            Description = "Returns risk metrics. Default returns a 2D grid; pass a metric name for a scalar.",
            Category = "Convex Risk")]
        public static object CxRisk(
            object bondRef,
            DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Curve handle (spread marks + KRD only)")] object curveRef,
            [ExcelArgument("Metric: grid (default) | mod_dur | mac_dur | convexity | dv01 | spread_dur | krd")] object metric,
            [ExcelArgument("Quote frequency, default SA")] object quoteFrequency,
            [ExcelArgument("Key-rate tenors (years) for KRD; range or csv string")] object keyRateTenors) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandle(bondRef, "bond"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                    ["mark"] = CxParse.AsMark(mark),
                    ["quote_frequency"] = CxParse.AsFrequency(quoteFrequency),
                };
                var curveHandle = CxParse.AsHandleOrNull(curveRef);
                if (curveHandle is ulong c) req["curve"] = c;

                var tenors = ParseTenors(keyRateTenors);
                if (tenors.Length > 0)
                {
                    var arr = new JArray();
                    foreach (var t in tenors) arr.Add(t);
                    req["key_rate_tenors"] = arr;
                }

                var result = Cx.Risk(req);
                return SelectRiskField(result, AsString(metric, "grid").ToLowerInvariant());
            });

        [ExcelFunction(Name = "CX.SPREAD",
            Description = "Computes a spread (Z, G, I, ASW, OAS, DM, …) at the given mark.",
            Category = "Convex Spreads")]
        public static object CxSpread(
            object bondRef,
            object curveRef,
            DateTime settlement,
            [ExcelArgument("Mark — see CX.PRICE for grammar")] object mark,
            [ExcelArgument("Spread: Z (default) | G | I | OAS | DM | ASW | ASW_PROC | CREDIT")] object spreadType,
            [ExcelArgument("Optional volatility for OAS, default 1%")] object volatility,
            [ExcelArgument("Field: bps (default) | grid")] object field) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandle(bondRef, "bond"),
                    ["curve"] = CxParse.AsHandle(curveRef, "curve"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                    ["mark"] = CxParse.AsMark(mark),
                    ["spread_type"] = CxParse.AsSpreadType(AsString(spreadType, "Z")),
                };
                if (!IsBlank(volatility))
                    req["params"] = new JObject { ["volatility"] = AsDouble(volatility, 0.01) / 100.0 };

                var result = Cx.Spread(req);
                return SelectSpreadField(result, AsString(field, "bps").ToLowerInvariant());
            });

        [ExcelFunction(Name = "CX.CASHFLOWS",
            Description = "Bond cashflow schedule on or after settlement.",
            Category = "Convex Bonds")]
        public static object CxCashflows(object bondRef, DateTime settlement) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandle(bondRef, "bond"),
                    ["settlement"] = CxParse.AsIsoDate(settlement),
                };
                var result = Cx.Cashflows(req);
                return CashflowsToGrid(result);
            });

        [ExcelFunction(Name = "CX.MW",
            Description = "Make-whole call price for a callable bond carrying a make-whole spread. " +
                          "Returns price (default), discount_rate, or spread_bps.",
            Category = "Convex Bonds")]
        public static object CxMakeWhole(
            object bondRef,
            [ExcelArgument("Hypothetical call date")] DateTime callDate,
            [ExcelArgument("Treasury par yield, decimal (0.05 = 5%)")] double treasuryRate,
            [ExcelArgument("Field: price (default) | discount_rate | spread_bps")] object field) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["bond"] = CxParse.AsHandle(bondRef, "bond"),
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
                    _ => throw new ConvexException($"unknown MW field: {f}"),
                };
            });

        [ExcelFunction(Name = "CX.CURVE.QUERY",
            Description = "Read a curve point: zero rate (default), discount factor, or forward rate.",
            Category = "Convex Curves")]
        public static object CxCurveQuery(
            object curveRef,
            [ExcelArgument("Tenor in years")] double tenor,
            [ExcelArgument("Query: zero (default) | df | forward")] object query,
            [ExcelArgument("End tenor (forward only)")] object tenorEnd) =>
            Safe(() =>
            {
                var req = new JObject
                {
                    ["curve"] = CxParse.AsHandle(curveRef, "curve"),
                    ["query"] = AsString(query, "zero").ToLowerInvariant(),
                    ["tenor"] = tenor,
                };
                if (!IsBlank(tenorEnd)) req["tenor_end"] = AsDouble(tenorEnd, tenor + 0.25);
                var result = Cx.CurveQuery(req);
                return (double?)result["value"] ?? throw new ConvexException("missing value");
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
        public static string CxClear() { try { Cx.ClearAll(); return "OK"; } catch (Exception ex) { return "ERROR: " + ex.Message; } }

        [ExcelFunction(Name = "CX.DESCRIBE", Description = "JSON description of a registered object.",
            Category = "Convex Utilities")]
        public static object CxDescribe(object handle) =>
            Safe(() => Cx.Describe(CxParse.AsHandle(handle, "handle")));

        // ===================================================================
        // Helpers
        // ===================================================================

        private static object Safe(Func<object> body)
        {
            try { return body(); }
            catch (ConvexException ex) { return "#ERROR: " + ex.Message; }
            catch (Exception ex) { return "#ERROR: " + ex.Message; }
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

        private static DateTime[] ExtractDates(object range)
        {
            if (range is double d) return new[] { DateTime.FromOADate(d) };
            if (range is object[,] grid)
            {
                int rows = grid.GetLength(0), cols = grid.GetLength(1);
                var list = new System.Collections.Generic.List<DateTime>(rows * cols);
                for (int r = 0; r < rows; r++)
                    for (int c = 0; c < cols; c++)
                    {
                        var cell = grid[r, c];
                        if (cell is double dd) list.Add(DateTime.FromOADate(dd));
                        else if (cell is DateTime dt) list.Add(dt);
                        else if (cell is string ss && DateTime.TryParse(ss, out var parsed)) list.Add(parsed);
                    }
                return list.ToArray();
            }
            return Array.Empty<DateTime>();
        }

        private static string[] ExtractStrings(object range)
        {
            if (range is string s) return new[] { s };
            if (range is object[,] grid)
            {
                int rows = grid.GetLength(0), cols = grid.GetLength(1);
                var list = new System.Collections.Generic.List<string>(rows * cols);
                for (int r = 0; r < rows; r++)
                    for (int c = 0; c < cols; c++)
                        if (grid[r, c] is string ss) list.Add(ss);
                        else if (grid[r, c] is not null) list.Add(grid[r, c]!.ToString()!);
                return list.ToArray();
            }
            return Array.Empty<string>();
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
                default: throw new ConvexException("unknown CX.PRICE field " + field);
            }
        }

        private static double[] ParseTenors(object cell)
        {
            if (IsBlank(cell)) return Array.Empty<double>();
            if (cell is string s)
            {
                return s.Split(new[] { ',', ';', ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries)
                    .Select(t =>
                        double.Parse(t.Trim(), NumberStyles.Any, CultureInfo.InvariantCulture))
                    .ToArray();
            }
            return CxParse.AsDoubles(cell);
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
                default: throw new ConvexException("unknown CX.RISK metric " + metric);
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
                default: throw new ConvexException("unknown CX.SPREAD field " + field);
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
