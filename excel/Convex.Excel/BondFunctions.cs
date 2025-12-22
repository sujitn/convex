using System;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Excel UDFs for bond creation and queries.
    /// All functions use the CX. prefix.
    /// </summary>
    public static class BondFunctions
    {
        /// <summary>
        /// Creates a fixed-rate bond with full specification.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND",
            Description = "Creates a fixed-rate bond with full specification",
            Category = "Convex Bonds",
            IsVolatile = false)]
        public static object CxBond(
            [ExcelArgument(Description = "ISIN or identifier")] object isin,
            [ExcelArgument(Description = "Coupon rate (%)")] double couponPercent,
            [ExcelArgument(Description = "Coupon frequency (1, 2, 4, 12)")] int frequency,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue,
            [ExcelArgument(Description = "Day count (0-5)")] object dayCount,
            [ExcelArgument(Description = "Business day convention (0-3)")] object bdc)
        {
            try
            {
                string identifier = (isin is ExcelMissing || isin is ExcelEmpty) ? null : isin?.ToString();
                int dc = (dayCount is ExcelMissing || dayCount is ExcelEmpty) ? 4 : Convert.ToInt32(dayCount);
                int bdcVal = (bdc is ExcelMissing || bdc is ExcelEmpty) ? 2 : Convert.ToInt32(bdc);

                var handle = ConvexWrapper.CreateFixedBond(
                    identifier,
                    couponPercent,
                    frequency,
                    maturity,
                    issue,
                    (ConvexWrapper.DayCount)dc,
                    (ConvexWrapper.BusinessDayConvention)bdcVal);

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
        /// Creates a US corporate bond (semi-annual, 30/360).
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.CORP",
            Description = "Creates a US corporate bond (semi-annual, 30/360)",
            Category = "Convex Bonds",
            IsVolatile = false)]
        public static object CxBondCorp(
            [ExcelArgument(Description = "ISIN or identifier")] object isin,
            [ExcelArgument(Description = "Coupon rate (%)")] double couponPercent,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue)
        {
            try
            {
                string identifier = (isin is ExcelMissing || isin is ExcelEmpty) ? null : isin?.ToString();

                var handle = ConvexWrapper.CreateUSCorporateBond(
                    identifier,
                    couponPercent,
                    maturity,
                    issue);

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
        /// Creates a US Treasury bond (semi-annual, ACT/ACT).
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.TSY",
            Description = "Creates a US Treasury bond (semi-annual, ACT/ACT)",
            Category = "Convex Bonds",
            IsVolatile = false)]
        public static object CxBondTsy(
            [ExcelArgument(Description = "CUSIP or identifier")] object cusip,
            [ExcelArgument(Description = "Coupon rate (%)")] double couponPercent,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue)
        {
            try
            {
                string identifier = (cusip is ExcelMissing || cusip is ExcelEmpty) ? null : cusip?.ToString();

                var handle = ConvexWrapper.CreateUSTreasuryBond(
                    identifier,
                    couponPercent,
                    maturity,
                    issue);

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
        /// Gets the accrued interest for a bond at settlement.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.ACCRUED",
            Description = "Gets accrued interest for a bond",
            Category = "Convex Bonds")]
        public static object CxBondAccrued(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double accrued = ConvexWrapper.GetAccruedInterest(handle, settlement);
                return double.IsNaN(accrued) ? (object)ExcelError.ExcelErrorValue : accrued;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the maturity date of a bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.MATURITY",
            Description = "Gets maturity date of a bond",
            Category = "Convex Bonds")]
        public static object CxBondMaturity(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                var maturity = ConvexWrapper.GetMaturityDate(handle);
                return maturity == DateTime.MinValue ? (object)ExcelError.ExcelErrorValue : maturity;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the coupon rate of a bond (as percentage).
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.COUPON",
            Description = "Gets coupon rate of a bond (%)",
            Category = "Convex Bonds")]
        public static object CxBondCoupon(
            [ExcelArgument(Description = "Bond handle or name")] object bondRef)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double coupon = ConvexWrapper.GetCouponRate(handle);
                if (double.IsNaN(coupon))
                    return ExcelError.ExcelErrorValue;

                // Convert from decimal (0.05) to percentage (5.0)
                return coupon * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Callable Bond Functions
        // ========================================================================

        /// <summary>
        /// Creates a callable bond with a single call date.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.CALLABLE",
            Description = "Creates a callable bond with single call date",
            Category = "Convex Bonds")]
        public static object CxBondCallable(
            [ExcelArgument(Description = "Bond identifier (e.g., CUSIP)")] string isin,
            [ExcelArgument(Description = "Coupon rate (%)")] double couponPercent,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue,
            [ExcelArgument(Description = "First call date")] DateTime callDate,
            [ExcelArgument(Description = "Call price (% of par, e.g., 102)")] double callPrice,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency,
            [ExcelArgument(Description = "Day count (0-5)")] object dayCount)
        {
            try
            {
                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 2 : Convert.ToInt32(frequency);
                int dc = (dayCount is ExcelMissing || dayCount is ExcelEmpty) ? 4 : Convert.ToInt32(dayCount);

                ulong handle = ConvexWrapper.CreateCallableBond(
                    isin, couponPercent, freq,
                    maturity, issue, callDate, callPrice,
                    (ConvexWrapper.DayCount)dc);

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
        /// Gets the yield to first call for a callable bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD.CALL",
            Description = "Calculates yield to first call (%)",
            Category = "Convex Pricing")]
        public static object CxYieldToCall(
            [ExcelArgument(Description = "Callable bond handle or name")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double ytc = ConvexWrapper.CalculateYieldToCall(handle, settlement, cleanPrice);
                if (double.IsNaN(ytc))
                    return ExcelError.ExcelErrorValue;

                // Convert from decimal (0.05) to percentage (5.0)
                return ytc * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the first call date of a callable bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.CALL.DATE",
            Description = "Gets first call date of a callable bond",
            Category = "Convex Bonds")]
        public static object CxBondCallDate(
            [ExcelArgument(Description = "Callable bond handle or name")] object bondRef)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                DateTime callDate = ConvexWrapper.GetFirstCallDate(handle);
                if (callDate == DateTime.MinValue)
                    return ExcelError.ExcelErrorValue;

                return callDate;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the first call price of a callable bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.CALL.PRICE",
            Description = "Gets first call price of a callable bond",
            Category = "Convex Bonds")]
        public static object CxBondCallPrice(
            [ExcelArgument(Description = "Callable bond handle or name")] object bondRef)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double callPrice = ConvexWrapper.GetFirstCallPrice(handle);
                return double.IsNaN(callPrice) ? (object)ExcelError.ExcelErrorValue : callPrice;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Zero Coupon Bond Functions
        // ========================================================================

        /// <summary>
        /// Creates a zero coupon bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.ZERO",
            Description = "Creates a zero coupon bond",
            Category = "Convex Bonds")]
        public static object CxBondZero(
            [ExcelArgument(Description = "Bond identifier")] object isin,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue,
            [ExcelArgument(Description = "Compounding (0=Annual, 1=Semi, 2=Quarterly, 3=Monthly, 4=Continuous)")] object compounding,
            [ExcelArgument(Description = "Day count (0-5)")] object dayCount)
        {
            try
            {
                string identifier = (isin is ExcelMissing || isin is ExcelEmpty) ? null : isin?.ToString();
                int comp = (compounding is ExcelMissing || compounding is ExcelEmpty) ? 1 : Convert.ToInt32(compounding);
                int dc = (dayCount is ExcelMissing || dayCount is ExcelEmpty) ? 2 : Convert.ToInt32(dayCount);

                ulong handle = NativeMethods.convex_bond_zero_coupon(
                    identifier,
                    maturity.Year, maturity.Month, maturity.Day,
                    issue.Year, issue.Month, issue.Day,
                    comp, dc, 0, 100.0);

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
        /// Creates a US Treasury Bill.
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.TBILL",
            Description = "Creates a US Treasury Bill (discount security)",
            Category = "Convex Bonds")]
        public static object CxBondTBill(
            [ExcelArgument(Description = "CUSIP or identifier")] object cusip,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue)
        {
            try
            {
                string identifier = (cusip is ExcelMissing || cusip is ExcelEmpty) ? null : cusip?.ToString();

                ulong handle = NativeMethods.convex_bond_us_tbill(
                    identifier,
                    maturity.Year, maturity.Month, maturity.Day,
                    issue.Year, issue.Month, issue.Day,
                    100.0);

                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Floating Rate Note Functions
        // ========================================================================

        /// <summary>
        /// Creates a floating rate note (FRN).
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.FRN",
            Description = "Creates a floating rate note",
            Category = "Convex Bonds")]
        public static object CxBondFRN(
            [ExcelArgument(Description = "Bond identifier")] object isin,
            [ExcelArgument(Description = "Spread over index (bps)")] double spreadBps,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue,
            [ExcelArgument(Description = "Frequency (1,2,4,12)")] object frequency,
            [ExcelArgument(Description = "Rate index (0=SOFR, 1=ESTR, 2=SONIA, etc.)")] object rateIndex,
            [ExcelArgument(Description = "Day count (0-5)")] object dayCount)
        {
            try
            {
                string identifier = (isin is ExcelMissing || isin is ExcelEmpty) ? null : isin?.ToString();
                int freq = (frequency is ExcelMissing || frequency is ExcelEmpty) ? 4 : Convert.ToInt32(frequency);
                int idx = (rateIndex is ExcelMissing || rateIndex is ExcelEmpty) ? 0 : Convert.ToInt32(rateIndex);
                int dc = (dayCount is ExcelMissing || dayCount is ExcelEmpty) ? 0 : Convert.ToInt32(dayCount);

                ulong handle = NativeMethods.convex_bond_frn(
                    identifier,
                    spreadBps,
                    maturity.Year, maturity.Month, maturity.Day,
                    issue.Year, issue.Month, issue.Day,
                    freq, idx, dc, 0, 100.0, 0.0, 0.0);

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
        /// Creates a US Treasury FRN (SOFR-based).
        /// </summary>
        [ExcelFunction(
            Name = "CX.BOND.TSYFRN",
            Description = "Creates a US Treasury FRN (SOFR-based)",
            Category = "Convex Bonds")]
        public static object CxBondTsyFRN(
            [ExcelArgument(Description = "CUSIP or identifier")] object cusip,
            [ExcelArgument(Description = "Spread over SOFR (bps)")] double spreadBps,
            [ExcelArgument(Description = "Maturity date")] DateTime maturity,
            [ExcelArgument(Description = "Issue date")] DateTime issue)
        {
            try
            {
                string identifier = (cusip is ExcelMissing || cusip is ExcelEmpty) ? null : cusip?.ToString();

                ulong handle = NativeMethods.convex_bond_us_treasury_frn(
                    identifier,
                    spreadBps,
                    maturity.Year, maturity.Month, maturity.Day,
                    issue.Year, issue.Month, issue.Day);

                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorValue;

                return HandleHelper.Format(handle);
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // Callable Bond Advanced Analytics
        // ========================================================================

        /// <summary>
        /// Calculates yield to worst for a callable bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.YIELD.WORST",
            Description = "Calculates yield to worst (minimum of YTM and all YTCs) (%)",
            Category = "Convex Pricing")]
        public static object CxYieldToWorst(
            [ExcelArgument(Description = "Callable bond handle")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                var result = ConvexWrapper.CalculateYieldToWorst(handle, settlement, cleanPrice);
                if (result == null)
                    return ExcelError.ExcelErrorValue;

                // Convert from decimal (0.05) to percentage (5.0)
                return result.Yield * 100.0;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Gets the workout date for yield to worst calculation.
        /// </summary>
        [ExcelFunction(
            Name = "CX.WORKOUT.DATE",
            Description = "Gets the workout date (call or maturity) for YTW",
            Category = "Convex Pricing")]
        public static object CxWorkoutDate(
            [ExcelArgument(Description = "Callable bond handle")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Clean price")] double cleanPrice)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                var result = ConvexWrapper.CalculateYieldToWorst(handle, settlement, cleanPrice);
                if (result == null)
                    return ExcelError.ExcelErrorValue;

                return result.WorkoutDate;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates OAS for a callable bond.
        /// </summary>
        [ExcelFunction(
            Name = "CX.OAS",
            Description = "Calculates Option-Adjusted Spread for callable bond (bps)",
            Category = "Convex Spreads")]
        public static object CxOAS(
            [ExcelArgument(Description = "Callable bond handle")] object bondRef,
            [ExcelArgument(Description = "Curve handle")] object curveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Dirty price")] double dirtyPrice,
            [ExcelArgument(Description = "Volatility (%)")] object volatility)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                ulong curveHandle = HandleHelper.Parse(curveRef);
                if (bondHandle == NativeMethods.INVALID_HANDLE || curveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double vol = (volatility is ExcelMissing || volatility is ExcelEmpty) ? 1.0 : Convert.ToDouble(volatility);
                vol = vol / 100.0; // Convert from % to decimal

                var result = ConvexWrapper.CalculateOASAnalytics(bondHandle, curveHandle, settlement, dirtyPrice, vol);
                if (result == null)
                    return ExcelError.ExcelErrorValue;

                return result.OasBps;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        // ========================================================================
        // FRN Analytics
        // ========================================================================

        /// <summary>
        /// Calculates simple margin for an FRN.
        /// </summary>
        [ExcelFunction(
            Name = "CX.SIMPLE.MARGIN",
            Description = "Calculates simple margin for FRN (bps)",
            Category = "Convex Spreads")]
        public static object CxSimpleMargin(
            [ExcelArgument(Description = "FRN handle")] object bondRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Dirty price")] double dirtyPrice,
            [ExcelArgument(Description = "Current index rate (%)")] double currentIndexPercent)
        {
            try
            {
                ulong handle = HandleHelper.Parse(bondRef);
                if (handle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double currentIndex = currentIndexPercent / 100.0; // Convert to decimal
                double margin = ConvexWrapper.CalculateSimpleMargin(handle, settlement, dirtyPrice, currentIndex);

                return double.IsNaN(margin) ? (object)ExcelError.ExcelErrorValue : margin;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }

        /// <summary>
        /// Calculates discount margin (Z-DM) for an FRN.
        /// </summary>
        [ExcelFunction(
            Name = "CX.DISCOUNT.MARGIN",
            Description = "Calculates discount margin (Z-DM) for FRN (bps)",
            Category = "Convex Spreads")]
        public static object CxDiscountMargin(
            [ExcelArgument(Description = "FRN handle")] object bondRef,
            [ExcelArgument(Description = "Forward curve handle")] object fwdCurveRef,
            [ExcelArgument(Description = "Discount curve handle")] object discCurveRef,
            [ExcelArgument(Description = "Settlement date")] DateTime settlement,
            [ExcelArgument(Description = "Dirty price")] double dirtyPrice)
        {
            try
            {
                ulong bondHandle = HandleHelper.Parse(bondRef);
                ulong fwdCurveHandle = HandleHelper.Parse(fwdCurveRef);
                ulong discCurveHandle = HandleHelper.Parse(discCurveRef);

                if (bondHandle == NativeMethods.INVALID_HANDLE ||
                    fwdCurveHandle == NativeMethods.INVALID_HANDLE ||
                    discCurveHandle == NativeMethods.INVALID_HANDLE)
                    return ExcelError.ExcelErrorRef;

                double dm = ConvexWrapper.CalculateDiscountMargin(
                    bondHandle, fwdCurveHandle, discCurveHandle, settlement, dirtyPrice);

                return double.IsNaN(dm) ? (object)ExcelError.ExcelErrorValue : dm;
            }
            catch (Exception ex)
            {
                return "#ERROR: " + ex.Message;
            }
        }
    }
}
