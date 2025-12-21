using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Excel UDFs for spread calculations (Z-spread, I-spread, G-spread).
    /// All functions use the CX. prefix.
    /// </summary>
    public static class SpreadFunctions
    {
        /// <summary>
        /// Calculates Z-spread for a bond given market price and discount curve.
        /// Z-spread is the constant spread over the spot curve that prices the bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ZSPREAD",
            Description = "Calculates Z-spread (constant spread over spot curve)",
            Category = "Convex Spreads")]
        public static object CxZSpread(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Discount curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                if (bondHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                ulong curveHandle = HandleHelper.Parse(curveRef);
                if (curveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double zSpread = ConvexWrapper.CalculateZSpread(bondHandle, curveHandle, settlement, cleanPrice);
                return double.IsNaN(zSpread) ? (object)ExcelError.ExcelErrorValue : zSpread;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates I-spread (interpolated swap spread) for a bond.
        /// I-spread is the difference between bond yield and swap rate at maturity.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ISPREAD",
            Description = "Calculates I-spread (spread over swap curve)",
            Category = "Convex Spreads")]
        public static object CxISpread(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Swap curve handle or name")] object swapCurveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Bond yield (decimal, e.g. 0.05 for 5%)")] double bondYield)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                if (bondHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                ulong curveHandle = HandleHelper.Parse(swapCurveRef);
                if (curveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double iSpread = ConvexWrapper.CalculateISpread(bondHandle, curveHandle, settlement, bondYield);
                return double.IsNaN(iSpread) ? (object)ExcelError.ExcelErrorValue : iSpread;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates G-spread (government spread) for a bond.
        /// G-spread is the difference between bond yield and government rate at maturity.
        /// </summary>
        [ExcelFunction(
            Name = "CX.GSPREAD",
            Description = "Calculates G-spread (spread over government curve)",
            Category = "Convex Spreads")]
        public static object CxGSpread(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Government curve handle or name")] object govtCurveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Bond yield (decimal, e.g. 0.05 for 5%)")] double bondYield)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                if (bondHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                ulong curveHandle = HandleHelper.Parse(govtCurveRef);
                if (curveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double gSpread = ConvexWrapper.CalculateGSpread(bondHandle, curveHandle, settlement, bondYield);
                return double.IsNaN(gSpread) ? (object)ExcelError.ExcelErrorValue : gSpread;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates Par-Par Asset Swap Spread for a bond.
        /// ASW is the spread over the swap curve that makes the asset swap package worth par.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ASW",
            Description = "Calculates Asset Swap Spread (par-par)",
            Category = "Convex Spreads")]
        public static object CxASW(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Swap curve handle or name")] object swapCurveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                if (bondHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                ulong curveHandle = HandleHelper.Parse(swapCurveRef);
                if (curveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double asw = ConvexWrapper.CalculateASWSpread(bondHandle, curveHandle, settlement, cleanPrice);
                return double.IsNaN(asw) ? (object)ExcelError.ExcelErrorValue : asw;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Returns Z-spread analytics as an array including spread, DV01, and duration.
        /// </summary>
        [ExcelFunction(
            Name = "CX.ZSPREAD.ANALYTICS",
            Description = "Returns Z-spread analytics (spread, DV01, duration)",
            Category = "Convex Spreads")]
        public static object CxZSpreadAnalytics(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Discount curve handle or name")] object curveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                if (bondHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                ulong curveHandle = HandleHelper.Parse(curveRef);
                if (curveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                var analytics = ConvexWrapper.CalculateZSpreadAnalytics(bondHandle, curveHandle, settlement, cleanPrice);
                if (analytics == null)
                    return ExcelError.ExcelErrorValue;

                // Return as a 2D array (vertical) with labels
                var result = new object[3, 2];
                result[0, 0] = "Z-Spread (bps)";
                result[0, 1] = analytics.SpreadBps;
                result[1, 0] = "Spread DV01";
                result[1, 1] = analytics.SpreadDv01;
                result[2, 0] = "Spread Duration";
                result[2, 1] = analytics.SpreadDuration;

                return result;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }
    }
}
