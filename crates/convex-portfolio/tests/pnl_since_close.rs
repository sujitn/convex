//! P&L since a desk close. Every expected amount is worked by hand from the prices given,
//! not by the function under test.

use convex_portfolio::pnl_since_close::{
    pnl_since_close, pnl_total_since_close, HeldPnl, HoldingSinceClose, NoPnlReason,
    PnlTotalSinceClose,
};
use convex_portfolio::PortfolioError;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn held(
    instrument_id: &str,
    close_face: Decimal,
    close_dirty_price: Decimal,
    face: Decimal,
    dirty_price: Decimal,
) -> HoldingSinceClose<'_> {
    HoldingSinceClose {
        instrument_id,
        close_face,
        close_dirty_price: Some(close_dirty_price),
        face,
        dirty_price: Some(dirty_price),
        entitled_cash: Some(Decimal::ZERO),
    }
}

#[test]
fn nothing_moves_nothing_is_made() {
    let book = pnl_since_close(&[
        held(
            "A",
            dec!(1000000),
            dec!(101.25),
            dec!(1000000),
            dec!(101.25),
        ),
        held("B", dec!(-250000), dec!(98.5), dec!(-250000), dec!(98.5)),
    ])
    .unwrap();
    assert_eq!(book.pnl, Decimal::ZERO);
    assert_eq!(book.entitled_cash, Decimal::ZERO);
    assert_eq!(book.excluded_count(), 0);
    for position in &book.positions {
        assert_eq!(position.held.unwrap().pnl, Decimal::ZERO);
    }
}

#[test]
fn a_long_gains_and_a_short_loses_as_prices_rise() {
    let book = pnl_since_close(&[
        // 1,000,000 × (101.50 − 101.25) / 100 = 2,500.
        held(
            "LONG",
            dec!(1000000),
            dec!(101.25),
            dec!(1000000),
            dec!(101.5),
        ),
        // −2,000,000 × (99.50 − 99.875) / 100 = 7,500.
        held(
            "SHORT",
            dec!(-2000000),
            dec!(99.875),
            dec!(-2000000),
            dec!(99.5),
        ),
    ])
    .unwrap();
    let ids: Vec<_> = book
        .positions
        .iter()
        .map(|p| p.instrument_id.as_str())
        .collect();
    assert_eq!(ids, ["LONG", "SHORT"]);
    assert_eq!(book.positions[0].held.unwrap().pnl, dec!(2500));
    assert_eq!(book.positions[1].held.unwrap().pnl, dec!(7500));
    assert_eq!(book.pnl, dec!(10000));
}

#[test]
fn a_coupon_between_the_close_and_now_is_not_a_loss() {
    // The dirty price falls from 102.45 to 100.02 as the 2.5 coupon is paid:
    // 1,000,000 × (100.02 − 102.45 + 2.5) / 100 = 700, of which cash 25,000.
    let long = HoldingSinceClose {
        entitled_cash: Some(dec!(2.5)),
        ..held(
            "LONG",
            dec!(1000000),
            dec!(102.45),
            dec!(1000000),
            dec!(100.02),
        )
    };
    // A short pays the coupon: −500,000 × 0.07 / 100 = −350, of which cash −12,500.
    let short = HoldingSinceClose {
        entitled_cash: Some(dec!(2.5)),
        ..held(
            "SHORT",
            dec!(-500000),
            dec!(102.45),
            dec!(-500000),
            dec!(100.02),
        )
    };
    let book = pnl_since_close(&[long, short]).unwrap();
    assert_eq!(
        book.positions[0].held.unwrap(),
        HeldPnl {
            held_face: dec!(1000000),
            pnl: dec!(700),
            entitled_cash: dec!(25000),
        }
    );
    assert_eq!(
        book.positions[1].held.unwrap(),
        HeldPnl {
            held_face: dec!(-500000),
            pnl: dec!(-350),
            entitled_cash: dec!(-12500),
        }
    );
    assert_eq!(book.pnl, dec!(350));
    assert_eq!(book.entitled_cash, dec!(12500));
}

#[test]
fn pnl_is_on_the_face_held_through() {
    let book = pnl_since_close(&[
        // Grown: P&L on the 1,000,000 held at the close, 1,000,000 × 0.5 / 100.
        held(
            "GROWN",
            dec!(1000000),
            dec!(100),
            dec!(1500000),
            dec!(100.5),
        ),
        // Cut: P&L on the 400,000 still held, 400,000 × 0.5 / 100.
        held("CUT", dec!(1000000), dec!(100), dec!(400000), dec!(100.5)),
        // A short cut: −250,000 × 0.5 / 100.
        held(
            "SHORT-CUT",
            dec!(-1000000),
            dec!(100),
            dec!(-250000),
            dec!(100.5),
        ),
        held(
            "FLIPPED",
            dec!(1000000),
            dec!(100),
            dec!(-1000000),
            dec!(100.5),
        ),
        held("ADDED", Decimal::ZERO, dec!(100), dec!(500000), dec!(100.5)),
        held(
            "REMOVED",
            dec!(500000),
            dec!(100),
            Decimal::ZERO,
            dec!(100.5),
        ),
    ])
    .unwrap();
    let position = |i: usize| book.positions[i].held;
    let held_pnl = |i: usize| position(i).unwrap();
    assert_eq!(
        held_pnl(0),
        HeldPnl {
            held_face: dec!(1000000),
            pnl: dec!(5000),
            entitled_cash: Decimal::ZERO,
        }
    );
    assert_eq!(held_pnl(1).held_face, dec!(400000));
    assert_eq!(held_pnl(1).pnl, dec!(2000));
    assert_eq!(held_pnl(2).held_face, dec!(-250000));
    assert_eq!(held_pnl(2).pnl, dec!(-1250));
    assert_eq!(position(3), Err(NoPnlReason::SideChanged));
    assert_eq!(position(4), Err(NoPnlReason::AddedSinceClose));
    assert_eq!(position(5), Err(NoPnlReason::RemovedSinceClose));
    assert_eq!(book.pnl, dec!(5750));
    assert_eq!(book.excluded_count(), 3);
}

#[test]
fn a_position_without_a_value_or_cash_has_no_pnl_and_is_counted() {
    let book = pnl_since_close(&[
        held(
            "PRICED",
            dec!(1000000),
            dec!(100),
            dec!(1000000),
            dec!(100.25),
        ),
        HoldingSinceClose {
            close_dirty_price: None,
            ..held(
                "NEW-AT-CLOSE",
                dec!(1000000),
                dec!(100),
                dec!(1000000),
                dec!(100.5),
            )
        },
        HoldingSinceClose {
            dirty_price: None,
            ..held(
                "STALE",
                dec!(1000000),
                dec!(100),
                dec!(1000000),
                dec!(100.5),
            )
        },
        HoldingSinceClose {
            entitled_cash: None,
            ..held(
                "FLOATER",
                dec!(1000000),
                dec!(100),
                dec!(1000000),
                dec!(100.5),
            )
        },
        // How the face changed is said before what is missing.
        HoldingSinceClose {
            close_dirty_price: None,
            ..held(
                "ADDED",
                Decimal::ZERO,
                dec!(100),
                dec!(1000000),
                dec!(100.5),
            )
        },
    ])
    .unwrap();
    let reasons: Vec<_> = book.positions.iter().map(|p| p.held.err()).collect();
    assert_eq!(
        reasons,
        [
            None,
            Some(NoPnlReason::UnvaluedAtClose),
            Some(NoPnlReason::UnvaluedNow),
            Some(NoPnlReason::CashNotProjected),
            Some(NoPnlReason::AddedSinceClose),
        ]
    );
    assert_eq!(book.pnl, dec!(2500));
    assert_eq!(book.excluded_count(), 4);
}

#[test]
fn the_total_is_exact() {
    // 1,000 positions of 1,000,001 face, each up one billionth of a point:
    // 1,000 × 1,000,001 × 0.000000001 / 100 = 0.01000001.
    let ids: Vec<String> = (0..1000).map(|i| format!("BOND-{i:04}")).collect();
    let holdings: Vec<_> = ids
        .iter()
        .map(|id| {
            held(
                id,
                dec!(1000001),
                dec!(99.123456789),
                dec!(1000001),
                dec!(99.123456790),
            )
        })
        .collect();
    let book = pnl_since_close(&holdings).unwrap();
    assert_eq!(book.pnl.to_string(), "0.01000001");
    assert_eq!(book.positions.len(), 1000);
}

#[test]
fn the_total_alone_is_the_listings_total() {
    // 1,000 positions, each with cash and a different move, some excluded.
    let ids: Vec<String> = (0..1000).map(|i| format!("BOND-{i:04}")).collect();
    let holdings: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let step = Decimal::from(i as u64) / dec!(7000);
            let holding = HoldingSinceClose {
                entitled_cash: Some(dec!(2.6875)),
                ..held(
                    id,
                    dec!(1000001),
                    dec!(99.123456789),
                    dec!(1000001) - Decimal::from(i as u64),
                    dec!(99.123456789) + step,
                )
            };
            match i % 50 {
                0 => HoldingSinceClose {
                    dirty_price: None,
                    ..holding
                },
                1 => HoldingSinceClose {
                    entitled_cash: None,
                    ..holding
                },
                _ => holding,
            }
        })
        .collect();
    let book = pnl_since_close(&holdings).unwrap();
    assert_eq!(
        pnl_total_since_close(&holdings).unwrap(),
        PnlTotalSinceClose {
            pnl: book.pnl,
            entitled_cash: book.entitled_cash,
            excluded_count: book.excluded_count(),
        }
    );
    assert_eq!(book.excluded_count(), 40);
    let a = held("A", dec!(100), dec!(100), dec!(100), dec!(100));
    assert!(matches!(
        pnl_total_since_close(&[a, a]),
        Err(PortfolioError::InvalidHolding { .. })
    ));
}

#[test]
fn invalid_holdings_are_refused() {
    let refused = |holdings: &[HoldingSinceClose<'_>], reason: &str| match pnl_since_close(holdings)
    {
        Err(PortfolioError::InvalidHolding { reason: got, .. }) => assert_eq!(got, reason),
        other => panic!("expected {reason}, got {other:?}"),
    };
    let a = held("A", dec!(100), dec!(100), dec!(100), dec!(100));
    refused(&[a, a], "given twice");
    refused(
        &[held(
            "A",
            Decimal::ZERO,
            dec!(100),
            Decimal::ZERO,
            dec!(100),
        )],
        "held neither at the close nor now",
    );
    refused(
        &[held("A", dec!(100), dec!(-0.01), dec!(100), dec!(100))],
        "a dirty price is negative",
    );
    refused(
        &[HoldingSinceClose {
            entitled_cash: Some(dec!(-2.5)),
            ..a
        }],
        "entitled cash is negative",
    );
    // Each position's 5 × 10^28 fits a Decimal; their sum does not.
    let huge = |id| {
        held(
            id,
            dec!(50000000000000000000000000000),
            Decimal::ZERO,
            dec!(50000000000000000000000000000),
            dec!(100),
        )
    };
    assert!(matches!(
        pnl_since_close(&[huge("A"), huge("B")]),
        Err(PortfolioError::InvalidPortfolio { .. })
    ));
}

#[test]
fn only_a_position_held_through_and_valued_at_both_needs_its_cash() {
    let through = held("A", dec!(100), dec!(100), dec!(50), dec!(101));
    assert!(through.needs_entitled_cash());
    // Whatever cash it was given.
    assert!(HoldingSinceClose {
        entitled_cash: None,
        ..through
    }
    .needs_entitled_cash());
    for holding in [
        held("A", Decimal::ZERO, dec!(100), dec!(50), dec!(101)),
        held("A", dec!(100), dec!(100), Decimal::ZERO, dec!(101)),
        held("A", dec!(100), dec!(100), dec!(-50), dec!(101)),
        HoldingSinceClose {
            close_dirty_price: None,
            ..through
        },
        HoldingSinceClose {
            dirty_price: None,
            ..through
        },
    ] {
        assert!(!holding.needs_entitled_cash(), "{holding:?}");
    }
}
