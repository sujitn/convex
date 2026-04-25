//! Quick probe — print Convex 30/360 year fractions against QL's.
//! Run: cargo run -p reconcile_bench --example probe_30_360

use convex_core::daycounts::{DayCount, Thirty360US};
use convex_core::types::Date;

fn main() {
    let valuation = Date::from_ymd(2025, 12, 31).unwrap();
    let dcc = Thirty360US;
    let cfs = [
        (2026, 3, 21),
        (2026, 9, 21),
        (2027, 3, 21),
        (2027, 9, 21),
        (2028, 3, 21),
        (2028, 9, 21),
    ];
    println!("Convex Thirty360US.year_fraction from {valuation}:");
    for (y, m, d) in cfs {
        let cf = Date::from_ymd(y, m, d).unwrap();
        let yf = dcc.year_fraction(valuation, cf);
        let dc = dcc.day_count(valuation, cf);
        println!("  {y:04}-{m:02}-{d:02}: days={dc}, yf={yf}");
    }
}
