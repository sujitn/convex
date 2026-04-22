using System;
using System.Diagnostics;

namespace Convex.Excel
{
    /// <summary>
    /// Utility wrapper for Excel UDF bodies: runs the passed lambda, returns the
    /// result on success, otherwise traces the full exception and returns an
    /// Excel-friendly "#ERROR: ..." string.
    ///
    /// This exists so every UDF doesn't repeat the same try/catch.
    /// </summary>
    internal static class ExcelErrorHelper
    {
        internal static object SafeCall(Func<object> body)
        {
            try
            {
                return body();
            }
            catch (Exception ex)
            {
                Trace.TraceError("{0}", ex);
                return "#ERROR: " + ex.Message;
            }
        }
    }

    /// <summary>
    /// Convention defaults shared across the UDF layer. Prefer referencing
    /// <see cref="DefaultCouponFrequency"/> over hardcoding `2` in signatures
    /// and wrapper calls.
    /// </summary>
    internal static class ConventionDefaults
    {
        /// <summary>Semi-annual — the most common coupon frequency across US/EU bonds.</summary>
        internal const int DefaultCouponFrequency = 2;
    }
}
