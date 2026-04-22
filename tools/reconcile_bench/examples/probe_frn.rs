//! Probe the Convex FRN schedule to see exactly which coupon dates the
//! library generates for quarterly issuance anchored at 2025-10-31.

use convex_bonds::instruments::FixedRateBond;
use convex_bonds::traits::Bond;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use rust_decimal_macros::dec;

fn main() {
    use convex_bonds::types::CalendarId;
    use convex_core::calendars::BusinessDayConvention;
    let bond = FixedRateBond::builder()
        .cusip_unchecked("UST_FRN_2Y_PROBE")
        .coupon_rate(dec!(0.0369))
        .issue_date(Date::from_ymd(2025, 10, 31).unwrap())
        .maturity(Date::from_ymd(2027, 10, 31).unwrap())
        .frequency(Frequency::Quarterly)
        .day_count(DayCountConvention::Act360)
        .currency(Currency::USD)
        .face_value(dec!(100))
        .calendar(CalendarId::new(""))
        .business_day_convention(BusinessDayConvention::Unadjusted)
        .end_of_month(true)
        .build()
        .expect("build FRN");

    let settle = Date::from_ymd(2025, 12, 31).unwrap();
    let cfs = bond.cash_flows(settle);
    println!("Convex FRN cashflows from {settle}:");
    for cf in cfs {
        let astart = cf.accrual_start.map(|d| d.to_string()).unwrap_or("?".into());
        let aend = cf.accrual_end.map(|d| d.to_string()).unwrap_or("?".into());
        println!(
            "  pay={}  amt={:.6}  accrual=[{}, {}]",
            cf.date, cf.amount, astart, aend
        );
    }
}
