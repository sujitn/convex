use convex_bonds::instruments::FixedRateBond;
use convex_bonds::traits::Bond;
use convex_bonds::types::CalendarId;
use convex_core::calendars::BusinessDayConvention;
use convex_core::daycounts::DayCountConvention;
use convex_core::types::{Currency, Date, Frequency};
use rust_decimal_macros::dec;

fn main() {
    for (label, eom) in [("EOM=false", false), ("EOM=true", true)] {
        let bond = FixedRateBond::builder()
            .cusip_unchecked("UST_5Y_PROBE")
            .coupon_rate(dec!(0.03875))
            .issue_date(Date::from_ymd(2023, 1, 3).unwrap())
            .maturity(Date::from_ymd(2027, 12, 31).unwrap())
            .frequency(Frequency::SemiAnnual)
            .day_count(DayCountConvention::ActActIcma)
            .currency(Currency::USD)
            .face_value(dec!(100))
            .calendar(CalendarId::new(""))
            .business_day_convention(BusinessDayConvention::Unadjusted)
            .end_of_month(eom)
            .build()
            .expect("build");

        let settle = Date::from_ymd(2025, 12, 31).unwrap();
        println!("--- {label} ---");
        for cf in bond.cash_flows(settle) {
            let astart = cf.accrual_start.map(|d| d.to_string()).unwrap_or("?".into());
            let aend = cf.accrual_end.map(|d| d.to_string()).unwrap_or("?".into());
            println!("  pay={} amt={:.4} accrual=[{}, {}]", cf.date, cf.amount, astart, aend);
        }
    }
}
