using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Excel UDFs for bond pricing, yield, and risk calculations.
    /// All functions use the CX. prefix.
    /// </summary>
    public static class PricingFunctions
    {
        // ========================================================================
        // Yield / Price Functions
        // ========================================================================

        /// <summary>
        /// Calculates yield to maturity from clean price.
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD",
            Description = "Calculates yield to maturity from clean price",
            Category = "Convex Pricing")]
        public static object CxYield(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                double ytm = ConvexWrapper.CalculateYield(handle, settlement, cleanPrice, freq);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                // Convert from decimal (0.05) to percentage (5.0)
                return ytm * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates true yield from clean price.
        /// True yield accounts for actual settlement mechanics and reinvestment assumptions.
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD.TRUE",
            Description = "Calculates true yield (academic/theoretical)",
            Category = "Convex Pricing")]
        public static object CxYieldTrue(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // Convention 1 = True Yield
                double ytm = ConvexWrapper.CalculateYieldWithConvention(handle, settlement, cleanPrice, freq, 1);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                return ytm * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates ISMA/ICMA yield from clean price.
        /// European bond market standard (annual compounding).
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD.ISMA",
            Description = "Calculates ISMA/ICMA yield (European standard)",
            Category = "Convex Pricing")]
        public static object CxYieldIsma(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 1 : Convert.ToInt32(frequency);

                // Convention 2 = ISMA
                double ytm = ConvexWrapper.CalculateYieldWithConvention(handle, settlement, cleanPrice, freq, 2);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                return ytm * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates simple yield from clean price.
        /// Japanese JGB-style simple yield (no compounding).
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD.SIMPLE",
            Description = "Calculates simple yield (Japanese JGB style)",
            Category = "Convex Pricing")]
        public static object CxYieldSimple(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // Convention 3 = Simple Yield
                double ytm = ConvexWrapper.CalculateYieldWithConvention(handle, settlement, cleanPrice, freq, 3);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                return ytm * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates clean price from yield.
        /// </summary>
        [ExcelFunction(
            Name = "CX.PRICE",
            Description = "Calculates clean price from yield",
            Category = "Convex Pricing")]
        public static object CxPrice(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Yield to maturity (%)")] double yieldPercent,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // Convert from percentage (5.0) to decimal (0.05)
                double ytmDecimal = yieldPercent / 100.0;
                double price = ConvexWrapper.CalculatePrice(handle, settlement, ytmDecimal, freq);
                return double.IsNaN(price) ? (object)ExcelError.ExcelErrorValue : price;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates dirty (full) price from yield.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DIRTY.PRICE",
            Description = "Calculates dirty (full) price from yield",
            Category = "Convex Pricing")]
        public static object CxDirtyPrice(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Yield to maturity (%)")] double yieldPercent,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // Convert from percentage (5.0) to decimal (0.05)
                double ytmDecimal = yieldPercent / 100.0;
                double price = ConvexWrapper.CalculateDirtyPrice(handle, settlement, ytmDecimal, freq);
                return double.IsNaN(price) ? (object)ExcelError.ExcelErrorValue : price;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Risk Functions
        // ========================================================================

        /// <summary>
        /// Calculates modified duration.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DURATION",
            Description = "Calculates modified duration",
            Category = "Convex Risk")]
        public static object CxDuration(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // First calculate YTM from clean price (Rust returns decimal)
                double ytm = ConvexWrapper.CalculateYield(handle, settlement, cleanPrice, freq);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                // Pass YTM (not clean price) to duration function
                double dur = ConvexWrapper.CalculateModifiedDuration(handle, settlement, ytm, freq);
                return double.IsNaN(dur) ? (object)ExcelError.ExcelErrorValue : dur;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates Macaulay duration.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DURATION.MAC",
            Description = "Calculates Macaulay duration",
            Category = "Convex Risk")]
        public static object CxDurationMac(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // First calculate YTM from clean price (Rust returns decimal)
                double ytm = ConvexWrapper.CalculateYield(handle, settlement, cleanPrice, freq);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                // Pass YTM (not clean price) to duration function
                double dur = ConvexWrapper.CalculateMacaulayDuration(handle, settlement, ytm, freq);
                return double.IsNaN(dur) ? (object)ExcelError.ExcelErrorValue : dur;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates convexity.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CONVEXITY",
            Description = "Calculates convexity",
            Category = "Convex Risk")]
        public static object CxConvexity(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // First calculate YTM from clean price (Rust returns decimal)
                double ytm = ConvexWrapper.CalculateYield(handle, settlement, cleanPrice, freq);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                // Pass YTM (not clean price) to convexity function
                double cvx = ConvexWrapper.CalculateConvexity(handle, settlement, ytm, freq);
                return double.IsNaN(cvx) ? (object)ExcelError.ExcelErrorValue : cvx;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates DV01 (dollar value of 1 basis point).
        /// </summary>
        [ExcelFunction(
            Name = "CX.DV01",
            Description = "Calculates DV01 (dollar value of 1bp)",
            Category = "Convex Risk")]
        public static object CxDv01(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                // First calculate YTM from clean price (Rust returns decimal)
                double ytm = ConvexWrapper.CalculateYield(handle, settlement, cleanPrice, freq);
                if (double.IsNaN(ytm))
                    return ExcelError.ExcelErrorValue;

                // Calculate accrued interest to get dirty price
                double accrued = ConvexWrapper.GetAccruedInterest(handle, settlement);
                double dirtyPrice = cleanPrice + (double.IsNaN(accrued) ? 0 : accrued);

                // Pass YTM and dirty price to DV01 function
                double dv01 = ConvexWrapper.CalculateDV01(handle, settlement, ytm, dirtyPrice, freq);
                return double.IsNaN(dv01) ? (object)ExcelError.ExcelErrorValue : dv01;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Comprehensive Analytics
        // ========================================================================

        /// <summary>
        /// Returns a complete analytics array for a bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ANALYTICS",
            Description = "Returns complete bond analytics as array",
            Category = "Convex Risk")]
        public static object CxAnalytics(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);

                var analytics = ConvexWrapper.CalculateAnalytics(handle, settlement, cleanPrice, freq);
                if (analytics == null)
                    return ExcelError.ExcelErrorValue;

                // Return as a 2D array (vertical) with labels
                var result = new object[8, 2];
                result[0, 0] = "Clean Price";
                result[0, 1] = analytics.CleanPrice;
                result[1, 0] = "Dirty Price";
                result[1, 1] = analytics.DirtyPrice;
                result[2, 0] = "Accrued Interest";
                result[2, 1] = analytics.AccruedInterest;
                result[3, 0] = "Yield to Maturity (%)";
                result[3, 1] = analytics.YieldToMaturity * 100.0; // Convert to percentage
                result[4, 0] = "Modified Duration";
                result[4, 1] = analytics.ModifiedDuration;
                result[5, 0] = "Macaulay Duration";
                result[5, 1] = analytics.MacaulayDuration;
                result[6, 0] = "Convexity";
                result[6, 1] = analytics.Convexity;
                result[7, 0] = "DV01";
                result[7, 1] = analytics.DV01;

                return result;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Utility Functions
        // ========================================================================

        /// <summary>
        /// Calculates day count fraction between two dates.
        /// </summary>
        [ExcelFunction(
            Name = "CX.YEARFRAC",
            Description = "Calculates day count fraction between dates",
            Category = "Convex Utilities")]
        public static object CxYearFrac(
            [ExcelArgument(Description = "Start date")] DateTime startDate,
            [ExcelArgument(Description = "End date")] DateTime endDate,
            [ExcelArgument(Description = "Day count (0-5)")] int dayCount)
        {
            try
            {
                double yf = ConvexWrapper.CalculateDayCountFraction(
                    startDate,
                    endDate,
                    (ConvexWrapper.DayCount)dayCount);

                return double.IsNaN(yf) ? (object)ExcelError.ExcelErrorValue : yf;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Cash Flow Functions
        // ========================================================================

        /// <summary>
        /// Returns the cash flow schedule for a bond.
        /// Returns a 2D array with dates and amounts.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CASHFLOWS",
            Description = "Returns bond cash flow schedule as array",
            Category = "Convex Bonds")]
        public static object CxCashFlows(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int count = ConvexWrapper.GetCashFlowCount(handle, settlement);
                if (count <= 0)
                    return ExcelError.ExcelErrorValue;

                // Create array with header row + data rows
                var result = new object[count + 1, 2];
                result[0, 0] = "Date";
                result[0, 1] = "Amount";

                for (int i = 0; i < count; i++)
                {
                    DateTime cfDate;
                    double cfAmount;
                    if (ConvexWrapper.GetCashFlow(handle, settlement, i, out cfDate, out cfAmount))
                    {
                        result[i + 1, 0] = cfDate;
                        result[i + 1, 1] = cfAmount;
                    }
                    else
                    {
                        result[i + 1, 0] = ExcelError.ExcelErrorNA;
                        result[i + 1, 1] = ExcelError.ExcelErrorNA;
                    }
                }

                return result;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Returns the count of remaining cash flows for a bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CASHFLOW.COUNT",
            Description = "Returns number of remaining cash flows",
            Category = "Convex Bonds")]
        public static object CxCashFlowCount(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                int count = ConvexWrapper.GetCashFlowCount(handle, settlement);
                return count >= 0 ? (object)count : ExcelError.ExcelErrorValue;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

    }
}
