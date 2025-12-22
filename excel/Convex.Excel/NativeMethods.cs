using System;
using System.Runtime.InteropServices;

namespace Convex.Excel
{
    /// <summary>
    /// P/Invoke declarations for the Convex FFI native library.
    /// All functions are declared as extern "C" with cdecl calling convention.
    /// </summary>
    internal static class NativeMethods
    {
        private const string DllName = "convex_ffi.dll";
        private const CallingConvention Convention = CallingConvention.Cdecl;

        // ========================================================================
        // Error Codes
        // ========================================================================

        public const int CONVEX_OK = 0;
        public const int CONVEX_ERROR = -1;
        public const int CONVEX_ERROR_INVALID_ARG = -2;
        public const int CONVEX_ERROR_NULL_PTR = -3;
        public const int CONVEX_ERROR_NOT_FOUND = -4;

        public const ulong INVALID_HANDLE = 0;

        // ========================================================================
        // Object Management
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_release(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_get_type(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_lookup([MarshalAs(UnmanagedType.LPStr)] string name);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_object_count();

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern void convex_clear_all();

        // Callback delegate for object enumeration
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        public delegate void ObjectEnumCallback(ulong handle, int objectType, IntPtr name);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern void convex_enumerate_objects(ObjectEnumCallback callback, int filterType);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_get_name(ulong handle, [Out] byte[] buffer, int bufferLen);

        // ========================================================================
        // Error Handling
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern IntPtr convex_last_error_message();

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern void convex_clear_error();

        // ========================================================================
        // Version
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern IntPtr convex_version();

        // ========================================================================
        // Curve Functions
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_curve_from_zero_rates(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] double[] tenors,
            [In] double[] rates,
            int count,
            int interpolation,
            int dayCount);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_curve_from_dfs(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] double[] tenors,
            [In] double[] dfs,
            int count,
            int interpolation,
            int dayCount);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_curve_zero_rate(ulong handle, double tenor);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_curve_df(ulong handle, double tenor);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_curve_forward_rate(
            ulong handle, double startTenor, double endTenor);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_curve_ref_date(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_curve_tenor_count(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_curve_get_tenor(ulong handle, int index);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_curve_get_rate(ulong handle, int index);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_curve_max_tenor(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_curve_shift(
            ulong handle,
            double basisPoints,
            [MarshalAs(UnmanagedType.LPStr)] string newName);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_curve_twist(
            ulong handle,
            double shortBp,
            double longBp,
            double pivotTenor,
            [MarshalAs(UnmanagedType.LPStr)] string newName);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_curve_bump_tenor(
            ulong handle,
            double tenor,
            double basisPoints,
            [MarshalAs(UnmanagedType.LPStr)] string newName);

        // ========================================================================
        // Bond Functions
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_fixed(
            [MarshalAs(UnmanagedType.LPStr)] string isin,
            double couponRate,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay,
            int frequency,
            int dayCount,
            int currency,
            double faceValue);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_us_corporate(
            [MarshalAs(UnmanagedType.LPStr)] string isin,
            double couponPercent,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_us_treasury(
            [MarshalAs(UnmanagedType.LPStr)] string cusip,
            double couponPercent,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_accrued(
            ulong handle,
            int settleYear, int settleMonth, int settleDay);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_cashflow_count(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_maturity(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_coupon_rate(ulong handle);

        // ========================================================================
        // Callable Bond Functions
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_callable(
            [MarshalAs(UnmanagedType.LPStr)] string isin,
            double couponPercent,
            int frequency,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay,
            int callYear, int callMonth, int callDay,
            double callPrice,
            int dayCount);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_callable_schedule(
            [MarshalAs(UnmanagedType.LPStr)] string isin,
            double couponPercent,
            int frequency,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay,
            [In] int[] callDates,
            [In] double[] callPrices,
            int callCount,
            int dayCount);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_yield_to_call(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_first_call_date(ulong handle);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_first_call_price(ulong handle);

        // ========================================================================
        // Zero Coupon Bond Functions
        // ========================================================================

        /// <summary>
        /// Creates a zero coupon bond.
        /// </summary>
        /// <param name="isin">Bond identifier (can be null)</param>
        /// <param name="maturityYear">Maturity date year</param>
        /// <param name="maturityMonth">Maturity date month</param>
        /// <param name="maturityDay">Maturity date day</param>
        /// <param name="issueYear">Issue date year</param>
        /// <param name="issueMonth">Issue date month</param>
        /// <param name="issueDay">Issue date day</param>
        /// <param name="compounding">Compounding convention (0=Annual, 1=Semi, 2=Quarterly, 3=Monthly, 4=Continuous)</param>
        /// <param name="dayCount">Day count convention</param>
        /// <param name="currency">Currency index</param>
        /// <param name="faceValue">Face value (typically 100)</param>
        /// <returns>Handle to the zero coupon bond, or INVALID_HANDLE on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_zero_coupon(
            [MarshalAs(UnmanagedType.LPStr)] string isin,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay,
            int compounding,
            int dayCount,
            int currency,
            double faceValue);

        /// <summary>
        /// Creates a US Treasury Bill (zero coupon with T-Bill conventions).
        /// </summary>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_us_tbill(
            [MarshalAs(UnmanagedType.LPStr)] string cusip,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay,
            double faceValue);

        // ========================================================================
        // Floating Rate Note Functions
        // ========================================================================

        /// <summary>
        /// Creates a floating rate note (FRN).
        /// </summary>
        /// <param name="isin">Bond identifier (can be null)</param>
        /// <param name="spreadBps">Spread over reference rate in basis points</param>
        /// <param name="maturityYear">Maturity date year</param>
        /// <param name="maturityMonth">Maturity date month</param>
        /// <param name="maturityDay">Maturity date day</param>
        /// <param name="issueYear">Issue date year</param>
        /// <param name="issueMonth">Issue date month</param>
        /// <param name="issueDay">Issue date day</param>
        /// <param name="frequency">Payment frequency (1=Annual, 2=Semi, 4=Quarterly, 12=Monthly)</param>
        /// <param name="rateIndex">Reference rate index (0=SOFR, 1=ESTR, 2=SONIA, etc.)</param>
        /// <param name="dayCount">Day count convention</param>
        /// <param name="currency">Currency index</param>
        /// <param name="faceValue">Face value (typically 100)</param>
        /// <param name="capRate">Interest rate cap as decimal (0 for no cap)</param>
        /// <param name="floorRate">Interest rate floor as decimal (0 for no floor)</param>
        /// <returns>Handle to the FRN, or INVALID_HANDLE on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_frn(
            [MarshalAs(UnmanagedType.LPStr)] string isin,
            double spreadBps,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay,
            int frequency,
            int rateIndex,
            int dayCount,
            int currency,
            double faceValue,
            double capRate,
            double floorRate);

        /// <summary>
        /// Creates a US Treasury FRN (SOFR-based).
        /// </summary>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bond_us_treasury_frn(
            [MarshalAs(UnmanagedType.LPStr)] string cusip,
            double spreadBps,
            int maturityYear, int maturityMonth, int maturityDay,
            int issueYear, int issueMonth, int issueDay);

        // ========================================================================
        // Advanced Callable Bond Functions
        // ========================================================================

        /// <summary>
        /// Calculates yield to worst for a callable bond.
        /// Returns the minimum yield across all possible workout dates.
        /// </summary>
        /// <param name="handle">Callable bond handle</param>
        /// <param name="settleYear">Settlement date year</param>
        /// <param name="settleMonth">Settlement date month</param>
        /// <param name="settleDay">Settlement date day</param>
        /// <param name="cleanPrice">Clean price per 100 face value</param>
        /// <param name="yieldOut">Output: YTW as decimal (e.g., 0.05 for 5%)</param>
        /// <param name="dateOut">Output: Workout date as YYYYMMDD</param>
        /// <param name="priceOut">Output: Redemption price</param>
        /// <returns>CONVEX_OK on success, error code on failure</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_yield_to_worst(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice,
            out double yieldOut,
            out int dateOut,
            out double priceOut);

        /// <summary>
        /// Checks if a callable bond is callable on a specific date.
        /// </summary>
        /// <returns>1 if callable, 0 if not callable, -1 on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_is_callable_on(
            ulong handle,
            int dateYear, int dateMonth, int dateDay);

        /// <summary>
        /// Gets the number of call schedule entries.
        /// </summary>
        /// <returns>Number of entries, or -1 on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_call_schedule_count(ulong handle);

        /// <summary>
        /// Gets a specific call schedule entry.
        /// </summary>
        /// <param name="handle">Callable bond handle</param>
        /// <param name="index">Entry index (0-based)</param>
        /// <param name="dateOut">Output: Call date as YYYYMMDD</param>
        /// <param name="priceOut">Output: Call price</param>
        /// <returns>CONVEX_OK on success, error code on failure</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_call_schedule_entry(
            ulong handle,
            int index,
            out int dateOut,
            out double priceOut);

        /// <summary>
        /// Gets the call price on a specific date.
        /// </summary>
        /// <returns>Call price as percentage of par, or NaN if not callable on that date</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_call_price_on(
            ulong handle,
            int dateYear, int dateMonth, int dateDay);

        // ========================================================================
        // Pricing Functions
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_yield(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice,
            int frequency);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_yield_with_convention(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice,
            int frequency,
            int convention);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_cashflow_count(
            ulong handle,
            int settleYear, int settleMonth, int settleDay);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_cashflow_get(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            int index,
            out int dateOut,
            out double amountOut);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_price(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double yieldPercent,
            int frequency);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_dirty_price(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double yieldPercent,
            int frequency);

        // ========================================================================
        // Risk Functions
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_duration(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double ytm,
            int frequency);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_duration_macaulay(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double ytm,
            int frequency);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_convexity(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double ytm,
            int frequency);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_bond_dv01(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double ytm,
            double dirtyPrice,
            int frequency);

        // ========================================================================
        // Comprehensive Analytics
        // ========================================================================

        [StructLayout(LayoutKind.Sequential)]
        public struct FfiBondAnalytics
        {
            public double CleanPrice;
            public double DirtyPrice;
            public double Accrued;
            public double YieldToMaturity;
            public double ModifiedDuration;
            public double MacaulayDuration;
            public double Convexity;
            public double Dv01;
        }

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_bond_analytics(
            ulong handle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice,
            int frequency,
            out FfiBondAnalytics result);

        // ========================================================================
        // Day Count Utilities
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_day_count_fraction(
            int startYear, int startMonth, int startDay,
            int endYear, int endMonth, int endDay,
            int convention);

        // ========================================================================
        // Spread Functions
        // ========================================================================

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_z_spread(
            ulong bondHandle,
            ulong curveHandle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_i_spread(
            ulong bondHandle,
            ulong swapCurveHandle,
            int settleYear, int settleMonth, int settleDay,
            double bondYield);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_g_spread(
            ulong bondHandle,
            ulong govtCurveHandle,
            int settleYear, int settleMonth, int settleDay,
            double bondYield);

        [StructLayout(LayoutKind.Sequential)]
        public struct FfiSpreadResult
        {
            public double SpreadBps;
            public double SpreadDv01;
            public double SpreadDuration;
            public int Success;
        }

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_z_spread_analytics(
            ulong bondHandle,
            ulong curveHandle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice,
            out FfiSpreadResult result);

        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_asw_spread(
            ulong bondHandle,
            ulong swapCurveHandle,
            int settleYear, int settleMonth, int settleDay,
            double cleanPrice);

        // ========================================================================
        // Effective Duration / Convexity (Finite Difference)
        // ========================================================================

        /// <summary>
        /// Calculates effective duration using finite differences.
        /// D_eff = (P- - P+) / (2 × P0 × Δy)
        /// </summary>
        /// <param name="priceUp">Price when yield increases</param>
        /// <param name="priceDown">Price when yield decreases</param>
        /// <param name="priceBase">Base price</param>
        /// <param name="bumpBps">Yield bump size in basis points</param>
        /// <returns>Effective duration, or NaN on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_effective_duration(
            double priceUp,
            double priceDown,
            double priceBase,
            double bumpBps);

        /// <summary>
        /// Calculates effective convexity using finite differences.
        /// C_eff = (P- + P+ - 2×P0) / (P0 × Δy²)
        /// </summary>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_effective_convexity(
            double priceUp,
            double priceDown,
            double priceBase,
            double bumpBps);

        // ========================================================================
        // Key Rate Duration Functions
        // ========================================================================

        /// <summary>
        /// Calculates key rate duration at a specific tenor.
        /// </summary>
        /// <param name="priceUp">Price when rate at tenor increases</param>
        /// <param name="priceDown">Price when rate at tenor decreases</param>
        /// <param name="priceBase">Base price</param>
        /// <param name="bumpBps">Rate bump size in basis points</param>
        /// <param name="tenor">Tenor in years</param>
        /// <returns>Key rate duration at specified tenor</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_key_rate_duration(
            double priceUp,
            double priceDown,
            double priceBase,
            double bumpBps,
            double tenor);

        /// <summary>
        /// Gets standard key rate tenors.
        /// </summary>
        /// <param name="tenorsOut">Array to receive tenors</param>
        /// <param name="maxCount">Maximum number of tenors</param>
        /// <returns>Number of tenors written</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_standard_key_rate_tenors(
            [MarshalAs(UnmanagedType.LPArray)] double[] tenorsOut,
            int maxCount);

        // ========================================================================
        // OAS (Option-Adjusted Spread) Functions
        // ========================================================================

        /// <summary>
        /// OAS analytics result structure.
        /// </summary>
        [StructLayout(LayoutKind.Sequential)]
        public struct FfiOasResult
        {
            public double OasBps;
            public double EffectiveDuration;
            public double EffectiveConvexity;
            public double OptionValue;
            public int Success;
        }

        /// <summary>
        /// Calculates OAS for a callable bond.
        /// </summary>
        /// <param name="bondHandle">Callable bond handle</param>
        /// <param name="curveHandle">Discount curve handle</param>
        /// <param name="settleYear">Settlement year</param>
        /// <param name="settleMonth">Settlement month</param>
        /// <param name="settleDay">Settlement day</param>
        /// <param name="dirtyPrice">Market dirty price</param>
        /// <param name="volatility">Interest rate volatility (e.g., 0.01 for 1%)</param>
        /// <returns>OAS in basis points, or NaN on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_oas(
            ulong bondHandle,
            ulong curveHandle,
            int settleYear, int settleMonth, int settleDay,
            double dirtyPrice,
            double volatility);

        /// <summary>
        /// Calculates comprehensive OAS analytics.
        /// </summary>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern int convex_oas_analytics(
            ulong bondHandle,
            ulong curveHandle,
            int settleYear, int settleMonth, int settleDay,
            double dirtyPrice,
            double volatility,
            out FfiOasResult result);

        // ========================================================================
        // Discount Margin Functions (FRNs)
        // ========================================================================

        /// <summary>
        /// Calculates simple margin for a floating rate note.
        /// </summary>
        /// <param name="frnHandle">FRN handle</param>
        /// <param name="settleYear">Settlement year</param>
        /// <param name="settleMonth">Settlement month</param>
        /// <param name="settleDay">Settlement day</param>
        /// <param name="dirtyPrice">Market dirty price</param>
        /// <param name="currentIndex">Current index rate as decimal (e.g., 0.05 for 5%)</param>
        /// <returns>Simple margin in basis points, or NaN on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_simple_margin(
            ulong frnHandle,
            int settleYear, int settleMonth, int settleDay,
            double dirtyPrice,
            double currentIndex);

        /// <summary>
        /// Calculates Z-DM (Zero Discount Margin) for a floating rate note.
        /// </summary>
        /// <param name="frnHandle">FRN handle</param>
        /// <param name="forwardCurveHandle">Forward curve handle</param>
        /// <param name="discountCurveHandle">Discount curve handle</param>
        /// <param name="settleYear">Settlement year</param>
        /// <param name="settleMonth">Settlement month</param>
        /// <param name="settleDay">Settlement day</param>
        /// <param name="dirtyPrice">Market dirty price</param>
        /// <returns>Discount margin in basis points, or NaN on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern double convex_discount_margin(
            ulong frnHandle,
            ulong forwardCurveHandle,
            ulong discountCurveHandle,
            int settleYear, int settleMonth, int settleDay,
            double dirtyPrice);

        // ========================================================================
        // Curve Bootstrapping Functions
        // ========================================================================

        /// <summary>
        /// Bootstraps a curve from deposit and swap instruments.
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refYear">Reference date year</param>
        /// <param name="refMonth">Reference date month</param>
        /// <param name="refDay">Reference date day</param>
        /// <param name="depositTenors">Array of deposit tenors in years</param>
        /// <param name="depositRates">Array of deposit rates as decimals</param>
        /// <param name="depositCount">Number of deposits</param>
        /// <param name="swapTenors">Array of swap tenors in years</param>
        /// <param name="swapRates">Array of swap rates as decimals</param>
        /// <param name="swapCount">Number of swaps</param>
        /// <param name="interpolation">Interpolation method (0=Linear, 1=LogLinear, 2=Cubic, 3=MonotoneConvex)</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bootstrap_from_instruments(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] double[] depositTenors,
            [In] double[] depositRates,
            int depositCount,
            [In] double[] swapTenors,
            [In] double[] swapRates,
            int swapCount,
            int interpolation,
            int dayCount);

        /// <summary>
        /// Bootstraps a curve from OIS instruments.
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refYear">Reference date year</param>
        /// <param name="refMonth">Reference date month</param>
        /// <param name="refDay">Reference date day</param>
        /// <param name="tenors">Array of OIS tenors in years</param>
        /// <param name="rates">Array of OIS rates as decimals</param>
        /// <param name="count">Number of OIS instruments</param>
        /// <param name="interpolation">Interpolation method</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bootstrap_ois(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] double[] tenors,
            [In] double[] rates,
            int count,
            int interpolation,
            int dayCount);

        /// <summary>
        /// Bootstraps a curve from mixed instrument types.
        /// Instrument types: 0=Deposit, 1=FRA, 2=Swap, 3=OIS
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refYear">Reference date year</param>
        /// <param name="refMonth">Reference date month</param>
        /// <param name="refDay">Reference date day</param>
        /// <param name="instrumentTypes">Array of instrument types</param>
        /// <param name="tenors">Array of tenors in years</param>
        /// <param name="rates">Array of rates as decimals</param>
        /// <param name="count">Number of instruments</param>
        /// <param name="interpolation">Interpolation method</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bootstrap_mixed(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] int[] instrumentTypes,
            [In] double[] tenors,
            [In] double[] rates,
            int count,
            int interpolation,
            int dayCount);

        // ========================================================================
        // Piecewise Bootstrapping (iterative bootstrap)
        // ========================================================================

        /// <summary>
        /// Bootstraps a curve using piecewise/iterative method with Brent root-finding.
        /// Each instrument is solved exactly, one at a time in maturity order.
        /// </summary>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bootstrap_piecewise(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] double[] depositTenors,
            [In] double[] depositRates,
            int depositCount,
            [In] double[] swapTenors,
            [In] double[] swapRates,
            int swapCount,
            int interpolation,
            int dayCount);

        /// <summary>
        /// Bootstraps a curve with mixed instrument types using piecewise method.
        /// </summary>
        [DllImport(DllName, CallingConvention = Convention)]
        public static extern ulong convex_bootstrap_piecewise_mixed(
            [MarshalAs(UnmanagedType.LPStr)] string name,
            int refYear, int refMonth, int refDay,
            [In] int[] instrumentTypes,
            [In] double[] tenors,
            [In] double[] rates,
            int count,
            int interpolation,
            int dayCount);
    }
}
