"""Convex - Fixed income analytics library.

A high-performance Python library for bond pricing, yield curve construction,
and fixed income risk analytics, powered by Rust.

Examples:
    >>> from datetime import date
    >>> from convex import FixedRateBond, Frequency
    >>>
    >>> bond = FixedRateBond(
    ...     coupon=0.05,
    ...     maturity=date(2030, 1, 15),
    ...     issue_date=date(2020, 1, 15),
    ...     frequency=Frequency.SEMI_ANNUAL,
    ... )
    >>> bond.coupon_rate
    0.05
"""

from convex._convex import (
    # Core types
    Date,
    Currency,
    Frequency,
    DayCount,
    # Bond types
    FixedRateBond,
    CashFlow,
    # Version
    __version__,
)

__all__ = [
    # Core types
    "Date",
    "Currency",
    "Frequency",
    "DayCount",
    # Bond types
    "FixedRateBond",
    "CashFlow",
    # Pandas helpers
    "cash_flows_to_dataframe",
    # Version
    "__version__",
]


def cash_flows_to_dataframe(bond, settlement):
    """Convert bond cash flows to a pandas DataFrame.

    Args:
        bond: A bond object (e.g., FixedRateBond)
        settlement: Settlement date (datetime.date, Date, or string)

    Returns:
        pandas.DataFrame with columns: date, amount, flow_type

    Raises:
        ImportError: If pandas is not installed

    Examples:
        >>> from datetime import date
        >>> from convex import FixedRateBond, cash_flows_to_dataframe
        >>>
        >>> bond = FixedRateBond.us_treasury(0.05, date(2030, 1, 15), date(2020, 1, 15))
        >>> df = cash_flows_to_dataframe(bond, date(2025, 6, 15))
        >>> print(df.head())
                 date  amount       flow_type
        0  2025-07-15    2.50          Coupon
        1  2026-01-15    2.50          Coupon
        ...
    """
    try:
        import pandas as pd
    except ImportError:
        raise ImportError(
            "pandas is required for cash_flows_to_dataframe(). "
            "Install it with: pip install pandas"
        )

    # Get cash flows from the bond
    cash_flows = bond.cash_flows(settlement)

    # Convert to DataFrame
    data = []
    for cf in cash_flows:
        # Convert PyDate to datetime.date for pandas
        cf_date = cf.date
        if hasattr(cf_date, "year"):
            from datetime import date as dt_date

            py_date = dt_date(cf_date.year, cf_date.month, cf_date.day)
        else:
            py_date = cf_date

        data.append(
            {
                "date": py_date,
                "amount": cf.amount,
                "flow_type": cf.flow_type,
            }
        )

    df = pd.DataFrame(data)
    if not df.empty:
        df["date"] = pd.to_datetime(df["date"])
    return df
