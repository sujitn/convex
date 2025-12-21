using System;
using System.Runtime.InteropServices;

namespace Convex.Excel
{
    /// <summary>
    /// High-level wrapper for Convex FFI functions with error handling and marshalling.
    /// </summary>
    public static class ConvexWrapper
    {
        // ========================================================================
        // Enums (matching Rust FFI)
        // ========================================================================

        public enum Interpolation
        {
            Linear = 0,
            LogLinear = 1,
            CubicSpline = 2,
            MonotoneConvex = 3
        }

        public enum DayCount
        {
            Act360 = 0,
            Act365Fixed = 1,
            ActActIsda = 2,
            ActActIcma = 3,
            Thirty360US = 4,
            Thirty360E = 5
        }

        public enum BusinessDayConvention
        {
            Unadjusted = 0,
            Following = 1,
            ModifiedFollowing = 2,
            Preceding = 3
        }

        public enum ObjectType
        {
            Unknown = 0,
            Curve = 1,
            FixedBond = 2,
            ZeroBond = 3,
            FloatingRateNote = 4,
            CallableBond = 5,
            CashFlows = 6,
            PriceResult = 7,
            RiskResult = 8,
            SpreadResult = 9,
            YasResult = 10
        }

        // ========================================================================
        // Error Handling
        // ========================================================================

        /// <summary>
        /// Gets the last error message from the native library.
        /// </summary>
        public static string GetLastError()
        {
            var ptr = NativeMethods.convex_last_error_message();
            if (ptr == IntPtr.Zero)
                return string.Empty;
            return Marshal.PtrToStringAnsi(ptr) ?? string.Empty;
        }

        /// <summary>
        /// Clears the last error.
        /// </summary>
        public static void ClearError()
        {
            NativeMethods.convex_clear_error();
        }

        /// <summary>
        /// Gets the library version string.
        /// </summary>
        public static string GetVersion()
        {
            var ptr = NativeMethods.convex_version();
            if (ptr == IntPtr.Zero)
                return "unknown";
            return Marshal.PtrToStringAnsi(ptr) ?? "unknown";
        }

        // ========================================================================
        // Object Management
        // ========================================================================

        /// <summary>
        /// Releases an object by handle.
        /// </summary>
        public static bool Release(ulong handle)
        {
            return NativeMethods.convex_release(handle) == NativeMethods.CONVEX_OK;
        }

        /// <summary>
        /// Gets the type of an object.
        /// </summary>
        public static ObjectType GetObjectType(ulong handle)
        {
            return (ObjectType)NativeMethods.convex_get_type(handle);
        }

        /// <summary>
        /// Looks up a handle by name.
        /// </summary>
        public static ulong Lookup(string name)
        {
            return NativeMethods.convex_lookup(name);
        }

        /// <summary>
        /// Gets the number of registered objects.
        /// </summary>
        public static int ObjectCount()
        {
            return NativeMethods.convex_object_count();
        }

        /// <summary>
        /// Clears all registered objects.
        /// </summary>
        public static void ClearAll()
        {
            NativeMethods.convex_clear_all();
        }

        // ========================================================================
        // Curve Functions
        // ========================================================================

        /// <summary>
        /// Creates a yield curve from zero rates.
        /// </summary>
        public static ulong CreateCurveFromZeroRates(
            string name,
            DateTime refDate,
            double[] tenors,
            double[] rates,
            Interpolation interpolation = Interpolation.Linear,
            DayCount dayCount = DayCount.Act365Fixed)
        {
            if (tenors.Length != rates.Length)
                throw new ArgumentException("Tenors and rates arrays must have the same length");

            return NativeMethods.convex_curve_from_zero_rates(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                tenors, rates, tenors.Length,
                (int)interpolation, (int)dayCount);
        }

        /// <summary>
        /// Creates a yield curve from discount factors.
        /// </summary>
        public static ulong CreateCurveFromDiscountFactors(
            string name,
            DateTime refDate,
            double[] tenors,
            double[] dfs,
            Interpolation interpolation = Interpolation.LogLinear,
            DayCount dayCount = DayCount.Act365Fixed)
        {
            if (tenors.Length != dfs.Length)
                throw new ArgumentException("Tenors and discount factors arrays must have the same length");

            return NativeMethods.convex_curve_from_dfs(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                tenors, dfs, tenors.Length,
                (int)interpolation, (int)dayCount);
        }

        /// <summary>
        /// Gets the zero rate at a given tenor.
        /// </summary>
        public static double GetZeroRate(ulong curveHandle, double tenor)
        {
            return NativeMethods.convex_curve_zero_rate(curveHandle, tenor);
        }

        /// <summary>
        /// Gets the discount factor at a given tenor.
        /// </summary>
        public static double GetDiscountFactor(ulong curveHandle, double tenor)
        {
            return NativeMethods.convex_curve_df(curveHandle, tenor);
        }

        /// <summary>
        /// Gets the forward rate between two tenors.
        /// </summary>
        public static double GetForwardRate(ulong curveHandle, double startTenor, double endTenor)
        {
            return NativeMethods.convex_curve_forward_rate(curveHandle, startTenor, endTenor);
        }

        /// <summary>
        /// Shifts a curve by a number of basis points (parallel shift).
        /// </summary>
        public static ulong ShiftCurve(ulong curveHandle, double basisPoints, string newName = null)
        {
            return NativeMethods.convex_curve_shift(curveHandle, basisPoints, newName);
        }

        /// <summary>
        /// Applies a twist (steepening/flattening) to a curve.
        /// </summary>
        public static ulong TwistCurve(ulong curveHandle, double shortBp, double longBp, double pivotTenor, string newName = null)
        {
            return NativeMethods.convex_curve_twist(curveHandle, shortBp, longBp, pivotTenor, newName);
        }

        /// <summary>
        /// Bumps a specific tenor on the curve.
        /// </summary>
        public static ulong BumpTenor(ulong curveHandle, double tenor, double basisPoints, string newName = null)
        {
            return NativeMethods.convex_curve_bump_tenor(curveHandle, tenor, basisPoints, newName);
        }

        // ========================================================================
        // Bond Functions
        // ========================================================================

        /// <summary>
        /// Creates a fixed-rate bond with full specification.
        /// </summary>
        public static ulong CreateFixedBond(
            string isin,
            double couponPercent,
            int frequency,
            DateTime maturity,
            DateTime issue,
            DayCount dayCount = DayCount.Thirty360US,
            BusinessDayConvention bdc = BusinessDayConvention.ModifiedFollowing)
        {
            return NativeMethods.convex_bond_fixed(
                isin,
                couponPercent / 100.0, // Convert from % to decimal
                maturity.Year, maturity.Month, maturity.Day,
                issue.Year, issue.Month, issue.Day,
                frequency,
                (int)dayCount,
                0,     // Currency: USD
                100.0); // Face value
        }

        /// <summary>
        /// Creates a US corporate bond (semi-annual, 30/360).
        /// </summary>
        public static ulong CreateUSCorporateBond(
            string isin,
            double couponPercent,
            DateTime maturity,
            DateTime issue)
        {
            return NativeMethods.convex_bond_us_corporate(
                isin, couponPercent,
                maturity.Year, maturity.Month, maturity.Day,
                issue.Year, issue.Month, issue.Day);
        }

        /// <summary>
        /// Creates a US Treasury bond (semi-annual, ACT/ACT).
        /// </summary>
        public static ulong CreateUSTreasuryBond(
            string cusip,
            double couponPercent,
            DateTime maturity,
            DateTime issue)
        {
            return NativeMethods.convex_bond_us_treasury(
                cusip, couponPercent,
                maturity.Year, maturity.Month, maturity.Day,
                issue.Year, issue.Month, issue.Day);
        }

        /// <summary>
        /// Gets the accrued interest for a bond at settlement.
        /// </summary>
        public static double GetAccruedInterest(ulong bondHandle, DateTime settlement)
        {
            return NativeMethods.convex_bond_accrued(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day);
        }

        /// <summary>
        /// Gets the maturity date of a bond as YYYYMMDD integer.
        /// </summary>
        public static DateTime GetMaturityDate(ulong bondHandle)
        {
            int dateInt = NativeMethods.convex_bond_maturity(bondHandle);
            if (dateInt <= 0)
                return DateTime.MinValue;

            int year = dateInt / 10000;
            int month = (dateInt / 100) % 100;
            int day = dateInt % 100;
            return new DateTime(year, month, day);
        }

        /// <summary>
        /// Gets the coupon rate of a bond (as percentage).
        /// </summary>
        public static double GetCouponRate(ulong bondHandle)
        {
            return NativeMethods.convex_bond_coupon_rate(bondHandle);
        }

        // ========================================================================
        // Callable Bond Functions
        // ========================================================================

        /// <summary>
        /// Creates a callable bond with a single call date.
        /// </summary>
        /// <param name="isin">Bond identifier</param>
        /// <param name="couponPercent">Annual coupon rate as percentage</param>
        /// <param name="frequency">Coupon frequency (1, 2, 4, 12)</param>
        /// <param name="maturity">Maturity date</param>
        /// <param name="issue">Issue date</param>
        /// <param name="callDate">First call date</param>
        /// <param name="callPrice">Call price as percentage of par (e.g., 102.0)</param>
        /// <param name="dayCount">Day count convention</param>
        public static ulong CreateCallableBond(
            string isin,
            double couponPercent,
            int frequency,
            DateTime maturity,
            DateTime issue,
            DateTime callDate,
            double callPrice,
            DayCount dayCount = DayCount.Thirty360US)
        {
            return NativeMethods.convex_bond_callable(
                isin, couponPercent, frequency,
                maturity.Year, maturity.Month, maturity.Day,
                issue.Year, issue.Month, issue.Day,
                callDate.Year, callDate.Month, callDate.Day,
                callPrice, (int)dayCount);
        }

        /// <summary>
        /// Creates a callable bond with multiple call dates.
        /// </summary>
        public static ulong CreateCallableBondSchedule(
            string isin,
            double couponPercent,
            int frequency,
            DateTime maturity,
            DateTime issue,
            DateTime[] callDates,
            double[] callPrices,
            DayCount dayCount = DayCount.Thirty360US)
        {
            if (callDates == null || callPrices == null || callDates.Length != callPrices.Length)
                return NativeMethods.INVALID_HANDLE;

            int[] dateInts = new int[callDates.Length];
            for (int i = 0; i < callDates.Length; i++)
            {
                dateInts[i] = callDates[i].Year * 10000 + callDates[i].Month * 100 + callDates[i].Day;
            }

            return NativeMethods.convex_bond_callable_schedule(
                isin, couponPercent, frequency,
                maturity.Year, maturity.Month, maturity.Day,
                issue.Year, issue.Month, issue.Day,
                dateInts, callPrices, callDates.Length, (int)dayCount);
        }

        /// <summary>
        /// Calculates yield to first call for a callable bond.
        /// </summary>
        public static double CalculateYieldToCall(ulong bondHandle, DateTime settlement, double cleanPrice)
        {
            return NativeMethods.convex_bond_yield_to_call(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice);
        }

        /// <summary>
        /// Gets the first call date of a callable bond.
        /// </summary>
        public static DateTime GetFirstCallDate(ulong bondHandle)
        {
            int dateInt = NativeMethods.convex_bond_first_call_date(bondHandle);
            if (dateInt <= 0)
                return DateTime.MinValue;

            int year = dateInt / 10000;
            int month = (dateInt / 100) % 100;
            int day = dateInt % 100;
            return new DateTime(year, month, day);
        }

        /// <summary>
        /// Gets the first call price of a callable bond.
        /// </summary>
        public static double GetFirstCallPrice(ulong bondHandle)
        {
            return NativeMethods.convex_bond_first_call_price(bondHandle);
        }

        // ========================================================================
        // Pricing Functions
        // ========================================================================

        /// <summary>
        /// Calculates bond yield from clean price (Street Convention).
        /// </summary>
        public static double CalculateYield(ulong bondHandle, DateTime settlement, double cleanPrice, int frequency = 2)
        {
            return NativeMethods.convex_bond_yield(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice, frequency);
        }

        /// <summary>
        /// Calculates bond yield with a specific convention.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="cleanPrice">Clean price per 100 face value</param>
        /// <param name="frequency">Compounding frequency (1, 2, 4, 12)</param>
        /// <param name="convention">Yield convention (0=Street, 1=True, 2=ISMA, 3=Simple, 4=Discount, 5=BEY, 6=Muni, 7=Continuous)</param>
        public static double CalculateYieldWithConvention(ulong bondHandle, DateTime settlement, double cleanPrice, int frequency, int convention)
        {
            return NativeMethods.convex_bond_yield_with_convention(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice, frequency, convention);
        }

        /// <summary>
        /// Gets the number of remaining cash flows for a bond.
        /// </summary>
        public static int GetCashFlowCount(ulong bondHandle, DateTime settlement)
        {
            return NativeMethods.convex_bond_cashflow_count(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day);
        }

        /// <summary>
        /// Gets a specific cash flow for a bond.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="index">Zero-based cash flow index</param>
        /// <param name="date">Output: Cash flow date</param>
        /// <param name="amount">Output: Cash flow amount</param>
        /// <returns>True if successful, false otherwise</returns>
        public static bool GetCashFlow(ulong bondHandle, DateTime settlement, int index, out DateTime date, out double amount)
        {
            int dateInt;
            int result = NativeMethods.convex_bond_cashflow_get(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                index,
                out dateInt,
                out amount);

            if (result == NativeMethods.CONVEX_OK && dateInt > 0)
            {
                // Convert YYYYMMDD to DateTime
                int year = dateInt / 10000;
                int month = (dateInt / 100) % 100;
                int day = dateInt % 100;
                date = new DateTime(year, month, day);
                return true;
            }
            else
            {
                date = DateTime.MinValue;
                amount = 0;
                return false;
            }
        }

        /// <summary>
        /// Calculates clean price from yield.
        /// </summary>
        public static double CalculatePrice(ulong bondHandle, DateTime settlement, double yieldPercent, int frequency = 2)
        {
            return NativeMethods.convex_bond_price(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                yieldPercent, frequency);
        }

        /// <summary>
        /// Calculates dirty price from yield.
        /// </summary>
        public static double CalculateDirtyPrice(ulong bondHandle, DateTime settlement, double yieldPercent, int frequency = 2)
        {
            return NativeMethods.convex_bond_dirty_price(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                yieldPercent, frequency);
        }

        // ========================================================================
        // Risk Functions
        // ========================================================================

        /// <summary>
        /// Calculates modified duration.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="ytm">Yield to maturity (as decimal, e.g., 0.05 for 5%)</param>
        /// <param name="frequency">Compounding frequency (default: 2 for semi-annual)</param>
        public static double CalculateModifiedDuration(ulong bondHandle, DateTime settlement, double ytm, int frequency = 2)
        {
            return NativeMethods.convex_bond_duration(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                ytm, frequency);
        }

        /// <summary>
        /// Calculates Macaulay duration.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="ytm">Yield to maturity (as decimal, e.g., 0.05 for 5%)</param>
        /// <param name="frequency">Compounding frequency (default: 2 for semi-annual)</param>
        public static double CalculateMacaulayDuration(ulong bondHandle, DateTime settlement, double ytm, int frequency = 2)
        {
            return NativeMethods.convex_bond_duration_macaulay(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                ytm, frequency);
        }

        /// <summary>
        /// Calculates convexity.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="ytm">Yield to maturity (as decimal, e.g., 0.05 for 5%)</param>
        /// <param name="frequency">Compounding frequency (default: 2 for semi-annual)</param>
        public static double CalculateConvexity(ulong bondHandle, DateTime settlement, double ytm, int frequency = 2)
        {
            return NativeMethods.convex_bond_convexity(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                ytm, frequency);
        }

        /// <summary>
        /// Calculates DV01 (dollar value of 1bp).
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="ytm">Yield to maturity (as decimal, e.g., 0.05 for 5%)</param>
        /// <param name="dirtyPrice">Dirty price per 100 face value</param>
        /// <param name="frequency">Compounding frequency (default: 2 for semi-annual)</param>
        public static double CalculateDV01(ulong bondHandle, DateTime settlement, double ytm, double dirtyPrice, int frequency = 2)
        {
            return NativeMethods.convex_bond_dv01(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                ytm, dirtyPrice, frequency);
        }

        // ========================================================================
        // Comprehensive Analytics
        // ========================================================================

        /// <summary>
        /// Bond analytics result container.
        /// </summary>
        public class BondAnalytics
        {
            public double CleanPrice { get; set; }
            public double DirtyPrice { get; set; }
            public double AccruedInterest { get; set; }
            public double YieldToMaturity { get; set; }
            public double ModifiedDuration { get; set; }
            public double MacaulayDuration { get; set; }
            public double Convexity { get; set; }
            public double DV01 { get; set; }
        }

        /// <summary>
        /// Calculates all analytics for a bond in one call.
        /// </summary>
        public static BondAnalytics CalculateAnalytics(ulong bondHandle, DateTime settlement, double cleanPrice, int frequency = 2)
        {
            NativeMethods.FfiBondAnalytics result;
            int status = NativeMethods.convex_bond_analytics(
                bondHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice, frequency, out result);

            if (status != NativeMethods.CONVEX_OK)
                return null;

            return new BondAnalytics
            {
                CleanPrice = result.CleanPrice,
                DirtyPrice = result.DirtyPrice,
                AccruedInterest = result.Accrued,
                YieldToMaturity = result.YieldToMaturity,
                ModifiedDuration = result.ModifiedDuration,
                MacaulayDuration = result.MacaulayDuration,
                Convexity = result.Convexity,
                DV01 = result.Dv01
            };
        }

        // ========================================================================
        // Day Count Utilities
        // ========================================================================

        /// <summary>
        /// Calculates day count fraction between two dates.
        /// </summary>
        public static double CalculateDayCountFraction(DateTime start, DateTime end, DayCount convention)
        {
            return NativeMethods.convex_day_count_fraction(
                start.Year, start.Month, start.Day,
                end.Year, end.Month, end.Day,
                (int)convention);
        }

        // ========================================================================
        // Spread Functions
        // ========================================================================

        /// <summary>
        /// Calculates Z-spread for a bond given market price.
        /// The Z-spread is the constant spread over the spot curve that prices the bond.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="curveHandle">Discount curve handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="cleanPrice">Clean price (as percentage of par)</param>
        /// <returns>Z-spread in basis points</returns>
        public static double CalculateZSpread(ulong bondHandle, ulong curveHandle, DateTime settlement, double cleanPrice)
        {
            return NativeMethods.convex_z_spread(
                bondHandle, curveHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice);
        }

        /// <summary>
        /// Calculates I-spread (interpolated swap spread) for a bond.
        /// The I-spread is the difference between bond yield and swap rate at maturity.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="swapCurveHandle">Swap curve handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="bondYield">Bond yield to maturity (as decimal, e.g., 0.05 for 5%)</param>
        /// <returns>I-spread in basis points</returns>
        public static double CalculateISpread(ulong bondHandle, ulong swapCurveHandle, DateTime settlement, double bondYield)
        {
            return NativeMethods.convex_i_spread(
                bondHandle, swapCurveHandle,
                settlement.Year, settlement.Month, settlement.Day,
                bondYield);
        }

        /// <summary>
        /// Calculates G-spread (government spread) for a bond.
        /// The G-spread is the difference between bond yield and government rate at maturity.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="govtCurveHandle">Government curve handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="bondYield">Bond yield to maturity (as decimal, e.g., 0.05 for 5%)</param>
        /// <returns>G-spread in basis points</returns>
        public static double CalculateGSpread(ulong bondHandle, ulong govtCurveHandle, DateTime settlement, double bondYield)
        {
            return NativeMethods.convex_g_spread(
                bondHandle, govtCurveHandle,
                settlement.Year, settlement.Month, settlement.Day,
                bondYield);
        }

        /// <summary>
        /// Spread analytics result container.
        /// </summary>
        public class SpreadAnalytics
        {
            public double SpreadBps { get; set; }
            public double SpreadDv01 { get; set; }
            public double SpreadDuration { get; set; }
        }

        /// <summary>
        /// Calculates Z-spread with full analytics including DV01 and duration.
        /// </summary>
        public static SpreadAnalytics CalculateZSpreadAnalytics(ulong bondHandle, ulong curveHandle, DateTime settlement, double cleanPrice)
        {
            NativeMethods.FfiSpreadResult result;
            int status = NativeMethods.convex_z_spread_analytics(
                bondHandle, curveHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice, out result);

            if (status != NativeMethods.CONVEX_OK || result.Success == 0)
                return null;

            return new SpreadAnalytics
            {
                SpreadBps = result.SpreadBps,
                SpreadDv01 = result.SpreadDv01,
                SpreadDuration = result.SpreadDuration
            };
        }

        /// <summary>
        /// Calculates Par-Par Asset Swap Spread for a bond.
        /// ASW is the spread over the swap curve that makes the asset swap package worth par.
        /// </summary>
        /// <param name="bondHandle">Bond handle</param>
        /// <param name="swapCurveHandle">Swap curve handle</param>
        /// <param name="settlement">Settlement date</param>
        /// <param name="cleanPrice">Clean price (as percentage of par)</param>
        /// <returns>Asset swap spread in basis points</returns>
        public static double CalculateASWSpread(ulong bondHandle, ulong swapCurveHandle, DateTime settlement, double cleanPrice)
        {
            return NativeMethods.convex_asw_spread(
                bondHandle, swapCurveHandle,
                settlement.Year, settlement.Month, settlement.Day,
                cleanPrice);
        }

        // ========================================================================
        // Curve Bootstrapping Functions
        // ========================================================================

        /// <summary>
        /// Instrument types for mixed bootstrapping.
        /// </summary>
        public enum InstrumentType
        {
            Deposit = 0,
            FRA = 1,
            Swap = 2,
            OIS = 3
        }

        /// <summary>
        /// Bootstraps a curve from deposit and swap instruments.
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refDate">Reference date</param>
        /// <param name="depositTenors">Deposit tenors in years</param>
        /// <param name="depositRates">Deposit rates as decimals (0.04 for 4%)</param>
        /// <param name="swapTenors">Swap tenors in years</param>
        /// <param name="swapRates">Swap rates as decimals (0.04 for 4%)</param>
        /// <param name="interpolation">Interpolation method</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        public static ulong BootstrapCurve(
            string name,
            DateTime refDate,
            double[] depositTenors,
            double[] depositRates,
            double[] swapTenors,
            double[] swapRates,
            Interpolation interpolation = Interpolation.Linear,
            DayCount dayCount = DayCount.Act360)
        {
            int depositCount = depositTenors?.Length ?? 0;
            int swapCount = swapTenors?.Length ?? 0;

            return NativeMethods.convex_bootstrap_from_instruments(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                depositTenors ?? new double[0],
                depositRates ?? new double[0],
                depositCount,
                swapTenors ?? new double[0],
                swapRates ?? new double[0],
                swapCount,
                (int)interpolation,
                (int)dayCount);
        }

        /// <summary>
        /// Bootstraps a curve from OIS instruments.
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refDate">Reference date</param>
        /// <param name="tenors">OIS tenors in years</param>
        /// <param name="rates">OIS rates as decimals (0.04 for 4%)</param>
        /// <param name="interpolation">Interpolation method</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        public static ulong BootstrapOISCurve(
            string name,
            DateTime refDate,
            double[] tenors,
            double[] rates,
            Interpolation interpolation = Interpolation.Linear,
            DayCount dayCount = DayCount.Act360)
        {
            if (tenors == null || rates == null || tenors.Length != rates.Length)
                return NativeMethods.INVALID_HANDLE;

            return NativeMethods.convex_bootstrap_ois(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                tenors, rates, tenors.Length,
                (int)interpolation, (int)dayCount);
        }

        /// <summary>
        /// Bootstraps a curve from mixed instrument types.
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refDate">Reference date</param>
        /// <param name="instrumentTypes">Array of instrument types</param>
        /// <param name="tenors">Array of tenors in years</param>
        /// <param name="rates">Array of rates as decimals</param>
        /// <param name="interpolation">Interpolation method</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        public static ulong BootstrapMixedCurve(
            string name,
            DateTime refDate,
            int[] instrumentTypes,
            double[] tenors,
            double[] rates,
            Interpolation interpolation = Interpolation.Linear,
            DayCount dayCount = DayCount.Act360)
        {
            if (instrumentTypes == null || tenors == null || rates == null)
                return NativeMethods.INVALID_HANDLE;
            if (instrumentTypes.Length != tenors.Length || tenors.Length != rates.Length)
                return NativeMethods.INVALID_HANDLE;

            return NativeMethods.convex_bootstrap_mixed(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                instrumentTypes, tenors, rates, tenors.Length,
                (int)interpolation, (int)dayCount);
        }

        /// <summary>
        /// Bootstraps a curve using piecewise/iterative method (iterative).
        /// Each instrument is solved exactly using Brent root-finding.
        /// </summary>
        /// <param name="name">Optional name for the curve</param>
        /// <param name="refDate">Reference date</param>
        /// <param name="depositTenors">Array of deposit tenors in years</param>
        /// <param name="depositRates">Array of deposit rates as decimals</param>
        /// <param name="swapTenors">Array of swap tenors in years</param>
        /// <param name="swapRates">Array of swap rates as decimals</param>
        /// <param name="interpolation">Interpolation method</param>
        /// <param name="dayCount">Day count convention</param>
        /// <returns>Handle to the bootstrapped curve, or INVALID_HANDLE on error</returns>
        public static ulong BootstrapPiecewise(
            string name,
            DateTime refDate,
            double[] depositTenors,
            double[] depositRates,
            double[] swapTenors,
            double[] swapRates,
            Interpolation interpolation = Interpolation.Linear,
            DayCount dayCount = DayCount.Act360)
        {
            int depositCount = (depositTenors?.Length ?? 0);
            int swapCount = (swapTenors?.Length ?? 0);

            return NativeMethods.convex_bootstrap_piecewise(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                depositTenors ?? Array.Empty<double>(),
                depositRates ?? Array.Empty<double>(),
                depositCount,
                swapTenors ?? Array.Empty<double>(),
                swapRates ?? Array.Empty<double>(),
                swapCount,
                (int)interpolation, (int)dayCount);
        }

        /// <summary>
        /// Bootstraps a curve from mixed instrument types using piecewise method.
        /// </summary>
        public static ulong BootstrapPiecewiseMixed(
            string name,
            DateTime refDate,
            int[] instrumentTypes,
            double[] tenors,
            double[] rates,
            Interpolation interpolation = Interpolation.Linear,
            DayCount dayCount = DayCount.Act360)
        {
            if (instrumentTypes == null || tenors == null || rates == null)
                return NativeMethods.INVALID_HANDLE;
            if (instrumentTypes.Length != tenors.Length || tenors.Length != rates.Length)
                return NativeMethods.INVALID_HANDLE;

            return NativeMethods.convex_bootstrap_piecewise_mixed(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                instrumentTypes, tenors, rates, tenors.Length,
                (int)interpolation, (int)dayCount);
        }
    }
}
