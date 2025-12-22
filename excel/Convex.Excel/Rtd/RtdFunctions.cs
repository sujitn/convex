using System;
using System.Linq;
using ExcelDna.Integration;

namespace Convex.Excel.Rtd
{
    /// <summary>
    /// RTD-enabled Excel functions for real-time streaming calculations.
    /// These functions automatically update when their inputs change,
    /// making them ideal for integration with Bloomberg BDP/BDH feeds.
    ///
    /// Usage: Use CX.*.RTD versions instead of regular CX.* functions
    /// when you need real-time updates from streaming data sources.
    /// </summary>
    public static class RtdFunctions
    {
        private const string RtdProgId = "Convex.RtdServer";

        #region Curve Functions

        /// <summary>
        /// Creates a yield curve with RTD support for real-time updates.
        /// When input rates change, all dependent calculations automatically update.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.RTD",
            Description = "Creates a yield curve with real-time update support",
            Category = "Convex RTD",
            IsVolatile = false,
            IsMacroType = true)]
        public static object CxCurveRtd(
            [ExcelArgument(Description = "Curve name (e.g., 'USD.SWAP')")] string name,
            [ExcelArgument(Description = "Reference date")] DateTime refDate,
            [ExcelArgument(Description = "Tenors in years (array)")] object tenorsRange,
            [ExcelArgument(Description = "Zero rates in % (array)")] object ratesRange,
            [ExcelArgument(Description = "Interpolation: 0=Linear, 1=LogLinear, 2=CubicSpline")] int interpolation,
            [ExcelArgument(Description = "Day count: 0=ACT/360, 1=ACT/365")] int dayCount)
        {
            try
            {
                // Convert ranges to arrays
                double[] tenors = ConvertToDoubleArray(tenorsRange);
                double[] rates = ConvertToDoubleArray(ratesRange);

                if (tenors == null || rates == null || tenors.Length != rates.Length)
                    return ExcelError.ExcelErrorValue;

                // Create the curve (this updates or creates new)
                ulong handle = ConvexWrapper.CreateCurveFromZeroRates(
                    name, refDate, tenors, rates,
                    (ConvexWrapper.Interpolation)interpolation,
                    (ConvexWrapper.DayCount)dayCount);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                // Store handle in topic manager
                TopicManager.StoreHandle("curve", name, handle);

                // Generate input hash for cache invalidation
                string inputHash = TopicManager.HashInputs(refDate, tenors, rates, interpolation, dayCount);

                // Notify dependents that this curve has changed
                TopicManager.NotifyChange($"curve:{name}");

                // Return RTD subscription
                string topic = $"handle:curve:{name}";
                return XlCall.RTD(RtdProgId, null, topic);
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Gets zero rate from curve with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.ZERO.RTD",
            Description = "Gets zero rate at tenor with real-time updates",
            Category = "Convex RTD")]
        public static object CxCurveZeroRtd(
            [ExcelArgument(Description = "Curve name")] string curveName,
            [ExcelArgument(Description = "Tenor in years")] double tenor)
        {
            try
            {
                var handle = TopicManager.GetHandle("curve", curveName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double rate = ConvexWrapper.GetZeroRate(handle.Value, tenor);
                return double.IsNaN(rate) ? (object)ExcelError.ExcelErrorValue : rate * 100.0;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        #endregion

        #region Bond Functions

        /// <summary>
        /// Creates a bond with RTD support for real-time updates.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.RTD",
            Description = "Creates a fixed-rate bond with real-time update support",
            Category = "Convex RTD",
            IsMacroType = true)]
        public static object CxBondRtd(
            [ExcelArgument(Description = "Bond identifier (ISIN/CUSIP)")] string identifier,
            [ExcelArgument(Description = "Coupon rate (%)")] double couponPct,
            [ExcelArgument(Description = "Coupon frequency (1=Annual, 2=Semi, 4=Quarterly)")] int frequency,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue,
            [ExcelArgument(Description = "Day count convention")] int dayCount,
            [ExcelArgument(Description = "Business day convention")] int businessDayConvention)
        {
            try
            {
                ulong handle = ConvexWrapper.CreateFixedBond(
                    identifier, couponPct / 100.0, frequency, maturity, issue,
                    (ConvexWrapper.DayCount)dayCount,
                    (ConvexWrapper.BusinessDayConvention)businessDayConvention);

                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                // Store handle
                TopicManager.StoreHandle("bond", identifier, handle);

                // Notify dependents
                TopicManager.NotifyChange($"bond:{identifier}");

                string topic = $"handle:bond:{identifier}";
                return XlCall.RTD(RtdProgId, null, topic);
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Creates a US corporate bond with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.CORP.RTD",
            Description = "Creates a US corporate bond with real-time update support",
            Category = "Convex RTD",
            IsMacroType = true)]
        public static object CxBondCorpRtd(
            [ExcelArgument(Description = "Bond identifier")] string identifier,
            [ExcelArgument(Description = "Coupon rate (%)")] double couponPct,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue)
        {
            try
            {
                ulong handle = ConvexWrapper.CreateUSCorporateBond(identifier, couponPct, maturity, issue);

                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                TopicManager.StoreHandle("bond", identifier, handle);
                TopicManager.NotifyChange($"bond:{identifier}");

                string topic = $"handle:bond:{identifier}";
                return XlCall.RTD(RtdProgId, null, topic);
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        #endregion

        #region Pricing Functions (RTD-enabled)

        /// <summary>
        /// Calculates yield to maturity with RTD support.
        /// Automatically recalculates when bond or price inputs change.
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD.RTD",
            Description = "Calculates YTM with real-time updates",
            Category = "Convex RTD")]
        public static object CxYieldRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double price,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double yield = ConvexWrapper.CalculateYield(handle.Value, settlement, price, freq);
                return double.IsNaN(yield) ? (object)ExcelError.ExcelErrorValue : yield;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates clean price from yield with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.PRICE.RTD",
            Description = "Calculates price from yield with real-time updates",
            Category = "Convex RTD")]
        public static object CxPriceRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Yield (%)")] double yieldPct,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double prc = ConvexWrapper.CalculatePrice(handle.Value, settlement, yieldPct / 100.0, freq);
                return double.IsNaN(prc) ? (object)ExcelError.ExcelErrorValue : prc;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        #endregion

        #region Risk Functions (RTD-enabled)

        /// <summary>
        /// Calculates modified duration with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DURATION.RTD",
            Description = "Calculates modified duration with real-time updates",
            Category = "Convex RTD")]
        public static object CxDurationRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double price,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                // First calculate yield from price, then use yield for duration
                double yield = ConvexWrapper.CalculateYield(handle.Value, settlement, price, freq);
                double dur = ConvexWrapper.CalculateModifiedDuration(handle.Value, settlement, yield, freq);
                return double.IsNaN(dur) ? (object)ExcelError.ExcelErrorValue : dur;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates convexity with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CONVEXITY.RTD",
            Description = "Calculates convexity with real-time updates",
            Category = "Convex RTD")]
        public static object CxConvexityRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double price,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                // First calculate yield from price, then use yield for convexity
                double yield = ConvexWrapper.CalculateYield(handle.Value, settlement, price, freq);
                double conv = ConvexWrapper.CalculateConvexity(handle.Value, settlement, yield, freq);
                return double.IsNaN(conv) ? (object)ExcelError.ExcelErrorValue : conv;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates DV01 with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DV01.RTD",
            Description = "Calculates DV01 with real-time updates",
            Category = "Convex RTD")]
        public static object CxDv01Rtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double price,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                // First calculate yield from price, then use yield for DV01
                double yield = ConvexWrapper.CalculateYield(handle.Value, settlement, price, freq);
                double dirtyPrice = ConvexWrapper.CalculateDirtyPrice(handle.Value, settlement, yield, freq);
                double dv01 = ConvexWrapper.CalculateDV01(handle.Value, settlement, yield, dirtyPrice, freq);
                return double.IsNaN(dv01) ? (object)ExcelError.ExcelErrorValue : dv01;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        #endregion

        #region Spread Functions (RTD-enabled)

        /// <summary>
        /// Calculates Z-spread with RTD support.
        /// Automatically updates when curve or price changes.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ZSPREAD.RTD",
            Description = "Calculates Z-spread with real-time updates",
            Category = "Convex RTD")]
        public static object CxZSpreadRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Curve name")] string curveName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double price)
        {
            try
            {
                var bondHandle = TopicManager.GetHandle("bond", bondName);
                var curveHandle = TopicManager.GetHandle("curve", curveName);

                if (!bondHandle.HasValue || !curveHandle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double zspread = ConvexWrapper.CalculateZSpread(
                    bondHandle.Value, curveHandle.Value, settlement, price);

                return double.IsNaN(zspread) ? (object)ExcelError.ExcelErrorValue : zspread;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates price from Z-spread with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.PRICE.ZSPREAD.RTD",
            Description = "Calculates price from Z-spread with real-time updates",
            Category = "Convex RTD")]
        public static object CxPriceFromZSpreadRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Curve name")] string curveName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Z-spread (bps)")] double zSpreadBps)
        {
            try
            {
                var bondHandle = TopicManager.GetHandle("bond", bondName);
                var curveHandle = TopicManager.GetHandle("curve", curveName);

                if (!bondHandle.HasValue || !curveHandle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double prc = ConvexWrapper.PriceFromZSpread(
                    bondHandle.Value, curveHandle.Value, settlement, zSpreadBps);

                return double.IsNaN(prc) ? (object)ExcelError.ExcelErrorValue : prc;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates I-spread with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ISPREAD.RTD",
            Description = "Calculates I-spread with real-time updates",
            Category = "Convex RTD")]
        public static object CxISpreadRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Swap curve name")] string curveName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Bond yield (decimal)")] double bondYield)
        {
            try
            {
                var bondHandle = TopicManager.GetHandle("bond", bondName);
                var curveHandle = TopicManager.GetHandle("curve", curveName);

                if (!bondHandle.HasValue || !curveHandle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double ispread = ConvexWrapper.CalculateISpread(
                    bondHandle.Value, curveHandle.Value, settlement, bondYield);

                return double.IsNaN(ispread) ? (object)ExcelError.ExcelErrorValue : ispread;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates G-spread with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.GSPREAD.RTD",
            Description = "Calculates G-spread with real-time updates",
            Category = "Convex RTD")]
        public static object CxGSpreadRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Government curve name")] string curveName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Bond yield (decimal)")] double bondYield)
        {
            try
            {
                var bondHandle = TopicManager.GetHandle("bond", bondName);
                var curveHandle = TopicManager.GetHandle("curve", curveName);

                if (!bondHandle.HasValue || !curveHandle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double gspread = ConvexWrapper.CalculateGSpread(
                    bondHandle.Value, curveHandle.Value, settlement, bondYield);

                return double.IsNaN(gspread) ? (object)ExcelError.ExcelErrorValue : gspread;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates asset swap spread with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ASW.RTD",
            Description = "Calculates asset swap spread with real-time updates",
            Category = "Convex RTD")]
        public static object CxAswRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Swap curve name")] string curveName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                var bondHandle = TopicManager.GetHandle("bond", bondName);
                var curveHandle = TopicManager.GetHandle("curve", curveName);

                if (!bondHandle.HasValue || !curveHandle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double asw = ConvexWrapper.CalculateASWSpread(
                    bondHandle.Value, curveHandle.Value, settlement, cleanPrice);

                return double.IsNaN(asw) ? (object)ExcelError.ExcelErrorValue : asw;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates Macaulay duration with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DURATION.MAC.RTD",
            Description = "Calculates Macaulay duration with real-time updates",
            Category = "Convex RTD")]
        public static object CxDurationMacRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double price,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double yield = ConvexWrapper.CalculateYield(handle.Value, settlement, price, freq);
                double dur = ConvexWrapper.CalculateMacaulayDuration(handle.Value, settlement, yield, freq);
                return double.IsNaN(dur) ? (object)ExcelError.ExcelErrorValue : dur;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates dirty price with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DIRTY.PRICE.RTD",
            Description = "Calculates dirty price with real-time updates",
            Category = "Convex RTD")]
        public static object CxDirtyPriceRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Yield (%)")] double yieldPct,
            [ExcelArgument(Description = "Frequency (default 2)")] object freqArg)
        {
            try
            {
                int freq = freqArg is double d ? (int)d : 2;

                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double prc = ConvexWrapper.CalculateDirtyPrice(handle.Value, settlement, yieldPct / 100.0, freq);
                return double.IsNaN(prc) ? (object)ExcelError.ExcelErrorValue : prc;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Calculates accrued interest with RTD support.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ACCRUED.RTD",
            Description = "Calculates accrued interest with real-time updates",
            Category = "Convex RTD")]
        public static object CxAccruedRtd(
            [ExcelArgument(Description = "Bond name")] string bondName,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement)
        {
            try
            {
                var handle = TopicManager.GetHandle("bond", bondName);
                if (!handle.HasValue)
                    return ExcelError.ExcelErrorRef;

                double accrued = ConvexWrapper.GetAccruedInterest(handle.Value, settlement);
                return double.IsNaN(accrued) ? (object)ExcelError.ExcelErrorValue : accrued;
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        #endregion

        #region Utility Functions

        /// <summary>
        /// Gets RTD server statistics.
        /// </summary>
        [ExcelFunction(
            Name = "CX.RTD.STATS",
            Description = "Gets RTD server statistics",
            Category = "Convex RTD")]
        public static object CxRtdStats()
        {
            try
            {
                var server = ConvexRtdServer.Instance;
                if (server == null)
                    return "RTD Server not running";

                var stats = server.GetStats();
                return $"Topics: {stats.totalTopics} (Curves: {stats.curveTopics}, Bonds: {stats.bondTopics}, Analytics: {stats.analyticsTopics})";
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Forces refresh of all RTD topics matching a pattern.
        /// </summary>
        [ExcelFunction(
            Name = "CX.RTD.REFRESH",
            Description = "Forces refresh of RTD topics",
            Category = "Convex RTD",
            IsMacroType = true)]
        public static object CxRtdRefresh(
            [ExcelArgument(Description = "Pattern to match (e.g., 'curve:' or 'bond:AAPL')")] string pattern)
        {
            try
            {
                TopicManager.NotifyChange(pattern);
                return "Refresh queued";
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Converts an Excel range to a double array.
        /// </summary>
        private static double[] ConvertToDoubleArray(object range)
        {
            if (range is double d)
                return new[] { d };

            if (range is object[,] arr2d)
            {
                var result = new double[arr2d.GetLength(0) * arr2d.GetLength(1)];
                int idx = 0;
                for (int i = 0; i < arr2d.GetLength(0); i++)
                {
                    for (int j = 0; j < arr2d.GetLength(1); j++)
                    {
                        if (arr2d[i, j] is double val)
                            result[idx++] = val;
                        else if (double.TryParse(arr2d[i, j]?.ToString(), out var parsed))
                            result[idx++] = parsed;
                        else
                            return null;
                    }
                }
                return result.Take(idx).ToArray();
            }

            if (range is object[] arr1d)
            {
                return arr1d
                    .Select(o => o is double v ? v : double.TryParse(o?.ToString(), out var p) ? p : double.NaN)
                    .Where(v => !double.IsNaN(v))
                    .ToArray();
            }

            return null;
        }

        #endregion
    }
}
