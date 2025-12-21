using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Excel UDFs for yield curve operations.
    /// All functions use the CX. prefix.
    /// </summary>
    public static class CurveFunctions
    {
        /// <summary>
        /// Creates a yield curve from zero rates.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE",
            Description = "Creates a yield curve from zero rates",
            Category = "Convex Curves",
            IsVolatile = false)]
        public static object CxCurve(
            [ExcelArgument(Description = "Optional curve name")] object name,
            [ExcelArgument(Description = "Reference date")] DateTime refDate,
            [ExcelArgument(Description = "Array of tenors (years)")] double[] tenors,
            [ExcelArgument(Description = "Array of zero rates (%)")] double[] rates,
            [ExcelArgument(Description = "Interpolation method (0-3)")] int interpolation,
            [ExcelArgument(Description = "Day count convention (0-5)")] int dayCount)
        {
            try
            {
                string curveName = (name is ExcelMissing || name is ExcelEmpty) ? null : name?.ToString();

                // Convert rates from percentage to decimal (4.5% -> 0.045)
                var decimalRates = new double[rates.Length];
                for (int i = 0; i < rates.Length; i++)
                {
                    decimalRates[i] = rates[i] / 100.0;
                }

                var handle = ConvexWrapper.CreateCurveFromZeroRates(
                    curveName,
                    refDate,
                    tenors,
                    decimalRates,
                    (ConvexWrapper.Interpolation)interpolation,
                    (ConvexWrapper.DayCount)dayCount);

                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Creates a yield curve from discount factors.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.DF",
            Description = "Creates a yield curve from discount factors",
            Category = "Convex Curves",
            IsVolatile = false)]
        public static object CxCurveFromDf(
            [ExcelArgument(Description = "Optional curve name")] object name,
            [ExcelArgument(Description = "Reference date")] DateTime refDate,
            [ExcelArgument(Description = "Array of tenors (years)")] double[] tenors,
            [ExcelArgument(Description = "Array of discount factors")] double[] dfs,
            [ExcelArgument(Description = "Interpolation method (0-3)")] int interpolation,
            [ExcelArgument(Description = "Day count convention (0-5)")] int dayCount)
        {
            try
            {
                string curveName = (name is ExcelMissing || name is ExcelEmpty) ? null : name?.ToString();

                var handle = ConvexWrapper.CreateCurveFromDiscountFactors(
                    curveName,
                    refDate,
                    tenors,
                    dfs,
                    (ConvexWrapper.Interpolation)interpolation,
                    (ConvexWrapper.DayCount)dayCount);

                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the zero rate from a curve at a specific tenor.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.ZERO",
            Description = "Gets zero rate from curve at tenor",
            Category = "Convex Curves")]
        public static object CxCurveZero(
            [ExcelArgument(Description = "Curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Tenor in years")] double tenor)
        {
            try
            {
                ulong handle = HandleHelper.Parse(curveRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double rate = ConvexWrapper.GetZeroRate(handle, tenor);
                // Return as percentage (4.5 for 4.5%)
                return double.IsNaN(rate) ? (object)ExcelError.ExcelErrorValue : rate * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the discount factor from a curve at a specific tenor.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.DISCOUNT",
            Description = "Gets discount factor from curve at tenor",
            Category = "Convex Curves")]
        public static object CxCurveDiscount(
            [ExcelArgument(Description = "Curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Tenor in years")] double tenor)
        {
            try
            {
                ulong handle = HandleHelper.Parse(curveRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double df = ConvexWrapper.GetDiscountFactor(handle, tenor);
                return double.IsNaN(df) ? (object)ExcelError.ExcelErrorValue : df;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the forward rate between two tenors.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.FORWARD",
            Description = "Gets forward rate between two tenors",
            Category = "Convex Curves")]
        public static object CxCurveForward(
            [ExcelArgument(Description = "Curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Start tenor in years")] double startTenor,
            [ExcelArgument(Description = "End tenor in years")] double endTenor)
        {
            try
            {
                ulong handle = HandleHelper.Parse(curveRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double fwd = ConvexWrapper.GetForwardRate(handle, startTenor, endTenor);
                // Return as percentage (4.5 for 4.5%)
                return double.IsNaN(fwd) ? (object)ExcelError.ExcelErrorValue : fwd * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Shifts a curve by a number of basis points (parallel shift).
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.SHIFT",
            Description = "Creates a parallel-shifted curve",
            Category = "Convex Curves")]
        public static object CxCurveShift(
            [ExcelArgument(Description = "Curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Shift in basis points")] double basisPoints,
            [ExcelArgument(Description = "Optional new curve name")] object newName)
        {
            try
            {
                ulong handle = HandleHelper.Parse(curveRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                string name = (newName is ExcelMissing || newName is ExcelEmpty) ? null : newName?.ToString();
                ulong newHandle = ConvexWrapper.ShiftCurve(handle, basisPoints, name);

                return newHandle == NativeMethods.INVALID_HANDLE
                    ? (object)ExcelError.ExcelErrorValue
                    : HandleHelper.Format(newHandle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Applies a twist transformation to a curve.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.TWIST",
            Description = "Creates a twisted (steepened/flattened) curve",
            Category = "Convex Curves")]
        public static object CxCurveTwist(
            [ExcelArgument(Description = "Curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Short-end shift in bp")] double shortBp,
            [ExcelArgument(Description = "Long-end shift in bp")] double longBp,
            [ExcelArgument(Description = "Pivot tenor in years")] double pivotTenor,
            [ExcelArgument(Description = "Optional new curve name")] object newName)
        {
            try
            {
                ulong handle = HandleHelper.Parse(curveRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                string name = (newName is ExcelMissing || newName is ExcelEmpty) ? null : newName?.ToString();
                ulong newHandle = ConvexWrapper.TwistCurve(handle, shortBp, longBp, pivotTenor, name);

                return newHandle == NativeMethods.INVALID_HANDLE
                    ? (object)ExcelError.ExcelErrorValue
                    : HandleHelper.Format(newHandle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Bumps a specific tenor on the curve.
        /// </summary>
        [ExcelFunction(
            Name = "CX.CURVE.BUMP",
            Description = "Bumps a specific tenor on the curve",
            Category = "Convex Curves")]
        public static object CxCurveBump(
            [ExcelArgument(Description = "Curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Tenor to bump (years)")] double tenor,
            [ExcelArgument(Description = "Bump in basis points")] double basisPoints,
            [ExcelArgument(Description = "Optional new curve name")] object newName)
        {
            try
            {
                ulong handle = HandleHelper.Parse(curveRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                string name = (newName is ExcelMissing || newName is ExcelEmpty) ? null : newName?.ToString();
                ulong newHandle = ConvexWrapper.BumpTenor(handle, tenor, basisPoints, name);

                return newHandle == NativeMethods.INVALID_HANDLE
                    ? (object)ExcelError.ExcelErrorValue
                    : HandleHelper.Format(newHandle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Curve Bootstrapping Functions
        // ========================================================================

        /// <summary>
        /// Bootstraps a yield curve from deposit and swap instruments.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOOTSTRAP",
            Description = "Bootstraps a yield curve from deposit and swap instruments",
            Category = "Convex Curves",
            IsVolatile = false)]
        public static object CxBootstrap(
            [ExcelArgument(Description = "Optional curve name")] object name,
            [ExcelArgument(Description = "Reference date")] DateTime refDate,
            [ExcelArgument(Description = "Deposit tenors (years)")] double[] depositTenors,
            [ExcelArgument(Description = "Deposit rates (decimal, e.g., 0.04 for 4%)")] double[] depositRates,
            [ExcelArgument(Description = "Swap tenors (years)")] double[] swapTenors,
            [ExcelArgument(Description = "Swap rates (decimal)")] double[] swapRates,
            [ExcelArgument(Description = "Interpolation (0=Linear, 1=LogLinear, 2=Cubic, 3=MonotoneConvex)")] int interpolation,
            [ExcelArgument(Description = "Day count (0=ACT/360, 1=ACT/365)")] int dayCount)
        {
            try
            {
                string curveName = (name is ExcelMissing || name is ExcelEmpty) ? null : name?.ToString();

                var handle = ConvexWrapper.BootstrapCurve(
                    curveName,
                    refDate,
                    depositTenors,
                    depositRates,
                    swapTenors,
                    swapRates,
                    (ConvexWrapper.Interpolation)interpolation,
                    (ConvexWrapper.DayCount)dayCount);

                if (handle == NativeMethods.INVALID_HANDLE)
                {
                    string error = ConvexWrapper.GetLastError();
                    return string.IsNullOrEmpty(error) ? ExcelError.ExcelErrorValue : (object)("#ERROR: " + error);
                }

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Bootstraps a yield curve from OIS instruments.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOOTSTRAP.OIS",
            Description = "Bootstraps a yield curve from OIS instruments",
            Category = "Convex Curves",
            IsVolatile = false)]
        public static object CxBootstrapOIS(
            [ExcelArgument(Description = "Optional curve name")] object name,
            [ExcelArgument(Description = "Reference date")] DateTime refDate,
            [ExcelArgument(Description = "OIS tenors (years)")] double[] tenors,
            [ExcelArgument(Description = "OIS rates (decimal, e.g., 0.04 for 4%)")] double[] rates,
            [ExcelArgument(Description = "Interpolation (0=Linear, 1=LogLinear, 2=Cubic, 3=MonotoneConvex)")] int interpolation,
            [ExcelArgument(Description = "Day count (0=ACT/360, 1=ACT/365)")] int dayCount)
        {
            try
            {
                string curveName = (name is ExcelMissing || name is ExcelEmpty) ? null : name?.ToString();

                var handle = ConvexWrapper.BootstrapOISCurve(
                    curveName,
                    refDate,
                    tenors,
                    rates,
                    (ConvexWrapper.Interpolation)interpolation,
                    (ConvexWrapper.DayCount)dayCount);

                if (handle == NativeMethods.INVALID_HANDLE)
                {
                    string error = ConvexWrapper.GetLastError();
                    return string.IsNullOrEmpty(error) ? ExcelError.ExcelErrorValue : (object)("#ERROR: " + error);
                }

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Bootstraps a yield curve from mixed instrument types.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOOTSTRAP.MIXED",
            Description = "Bootstraps a yield curve from mixed instrument types (0=Deposit, 1=FRA, 2=Swap, 3=OIS)",
            Category = "Convex Curves",
            IsVolatile = false)]
        public static object CxBootstrapMixed(
            [ExcelArgument(Description = "Optional curve name")] object name,
            [ExcelArgument(Description = "Reference date")] DateTime refDate,
            [ExcelArgument(Description = "Instrument types (0=Deposit, 1=FRA, 2=Swap, 3=OIS)")] double[] instrumentTypes,
            [ExcelArgument(Description = "Tenors (years)")] double[] tenors,
            [ExcelArgument(Description = "Rates (decimal)")] double[] rates,
            [ExcelArgument(Description = "Interpolation (0=Linear, 1=LogLinear, 2=Cubic, 3=MonotoneConvex)")] int interpolation,
            [ExcelArgument(Description = "Day count (0=ACT/360, 1=ACT/365)")] int dayCount)
        {
            try
            {
                string curveName = (name is ExcelMissing || name is ExcelEmpty) ? null : name?.ToString();

                // Convert double array to int array
                int[] types = new int[instrumentTypes.Length];
                for (int i = 0; i < instrumentTypes.Length; i++)
                    types[i] = (int)instrumentTypes[i];

                var handle = ConvexWrapper.BootstrapMixedCurve(
                    curveName,
                    refDate,
                    types,
                    tenors,
                    rates,
                    (ConvexWrapper.Interpolation)interpolation,
                    (ConvexWrapper.DayCount)dayCount);

                if (handle == NativeMethods.INVALID_HANDLE)
                {
                    string error = ConvexWrapper.GetLastError();
                    return string.IsNullOrEmpty(error) ? ExcelError.ExcelErrorValue : (object)("#ERROR: " + error);
                }

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }
    }
}
