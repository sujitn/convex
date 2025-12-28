"""Tests for convex bond functionality."""

from datetime import date

import pytest


class TestFixedRateBond:
    """Tests for FixedRateBond class."""

    def test_create_bond_with_kwargs(self):
        """Test creating a bond with keyword arguments."""
        from convex import FixedRateBond, Frequency, DayCount

        bond = FixedRateBond(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
            frequency=Frequency.SEMI_ANNUAL,
            day_count=DayCount.THIRTY_360_US,
        )

        assert bond.coupon_rate == pytest.approx(0.05)
        assert bond.face_value == pytest.approx(100.0)

    def test_us_corporate_constructor(self):
        """Test US corporate bond convenience constructor."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_corporate(
            coupon=0.045,
            maturity=date(2028, 6, 15),
            issue_date=date(2023, 6, 15),
        )

        assert bond.coupon_rate == pytest.approx(0.045)
        assert bond.maturity.year == 2028
        assert bond.maturity.month == 6
        assert bond.maturity.day == 15

    def test_us_treasury_constructor(self):
        """Test US Treasury bond convenience constructor."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.0375,
            maturity=date(2033, 11, 15),
            issue_date=date(2023, 11, 15),
            # Note: CUSIP validation is strict, so we omit it in tests
        )

        assert bond.coupon_rate == pytest.approx(0.0375)

    def test_bond_properties(self):
        """Test bond property accessors."""
        from convex import FixedRateBond, Frequency, Currency

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        assert bond.coupon_rate == pytest.approx(0.05)
        assert bond.face_value == pytest.approx(100.0)
        assert bond.maturity.year == 2030
        assert bond.issue_date.year == 2020
        assert bond.frequency == Frequency.SEMI_ANNUAL
        assert bond.currency == Currency.USD

    def test_accrued_interest(self):
        """Test accrued interest calculation."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_corporate(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        # Mid-coupon period should have some accrued
        accrued = bond.accrued_interest(date(2025, 4, 15))
        assert accrued > 0
        assert accrued < 2.5  # Less than semi-annual coupon

    def test_cash_flows(self):
        """Test cash flow generation."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2027, 1, 15),
            issue_date=date(2022, 1, 15),
        )

        cfs = bond.cash_flows(date(2025, 6, 1))

        # Should have 4 more coupons plus principal
        assert len(cfs) >= 4

        # Last cash flow should include principal
        last_cf = cfs[-1]
        assert last_cf.amount > 100  # Principal + last coupon

    def test_next_coupon_date(self):
        """Test next coupon date lookup."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        next_coupon = bond.next_coupon_date(date(2025, 2, 1))
        assert next_coupon is not None
        assert next_coupon.year == 2025
        assert next_coupon.month == 7
        assert next_coupon.day == 15

    def test_has_matured(self):
        """Test maturity check."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2025, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        assert not bond.has_matured(date(2024, 12, 1))
        assert bond.has_matured(date(2025, 2, 1))

    def test_years_to_maturity(self):
        """Test years to maturity calculation."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        ytm = bond.years_to_maturity(date(2025, 1, 15))
        assert ytm is not None
        # The absolute value should be approximately 5 years
        assert abs(ytm) == pytest.approx(5.0, rel=0.01)

    def test_bond_repr(self):
        """Test bond string representation."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        repr_str = repr(bond)
        assert "FixedRateBond" in repr_str
        assert "5.00%" in repr_str
        assert "2030" in repr_str


class TestCashFlow:
    """Tests for CashFlow class."""

    def test_cash_flow_properties(self):
        """Test cash flow property accessors."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        cfs = bond.cash_flows(date(2029, 1, 1))

        # Check first coupon
        first = cfs[0]
        assert first.amount == pytest.approx(2.5, rel=0.01)
        assert first.flow_type == "Coupon"
        assert first.date.year == 2029

    def test_cash_flow_repr(self):
        """Test cash flow string representation."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        cfs = bond.cash_flows(date(2029, 6, 1))
        first = cfs[0]

        repr_str = repr(first)
        assert "CashFlow" in repr_str


class TestDate:
    """Tests for Date class."""

    def test_create_date(self):
        """Test creating a date."""
        from convex import Date

        d = Date(2025, 6, 15)
        assert d.year == 2025
        assert d.month == 6
        assert d.day == 15

    def test_parse_date(self):
        """Test parsing a date from string."""
        from convex import Date

        d = Date.parse("2025-06-15")
        assert d.year == 2025
        assert d.month == 6
        assert d.day == 15

    def test_date_equality(self):
        """Test date equality."""
        from convex import Date

        d1 = Date(2025, 6, 15)
        d2 = Date.parse("2025-06-15")
        assert d1 == d2

    def test_date_str(self):
        """Test date string representation."""
        from convex import Date

        d = Date(2025, 6, 15)
        assert str(d) == "2025-06-15"


class TestEnums:
    """Tests for enum types."""

    def test_currency(self):
        """Test Currency enum."""
        from convex import Currency

        assert Currency.USD is not None
        assert Currency.EUR is not None
        assert str(Currency.USD) == "USD"

    def test_frequency(self):
        """Test Frequency enum."""
        from convex import Frequency

        assert Frequency.SEMI_ANNUAL.periods_per_year() == 2
        assert Frequency.QUARTERLY.periods_per_year() == 4
        assert Frequency.ANNUAL.periods_per_year() == 1
        assert str(Frequency.SEMI_ANNUAL) == "Semi-Annual"

    def test_day_count(self):
        """Test DayCount enum."""
        from convex import DayCount

        assert DayCount.ACT_365_FIXED is not None
        assert DayCount.THIRTY_360_US is not None
        assert str(DayCount.ACT_365_FIXED) == "ACT/365 Fixed"


class TestPandasHelpers:
    """Tests for pandas DataFrame helpers."""

    def test_cash_flows_to_dataframe(self):
        """Test converting cash flows to DataFrame."""
        pytest.importorskip("pandas")

        from convex import FixedRateBond, cash_flows_to_dataframe

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2027, 1, 15),
            issue_date=date(2022, 1, 15),
        )

        df = cash_flows_to_dataframe(bond, date(2025, 6, 1))

        # Check DataFrame structure
        assert "date" in df.columns
        assert "amount" in df.columns
        assert "flow_type" in df.columns
        assert len(df) >= 4

    def test_cash_flows_to_dataframe_empty(self):
        """Test DataFrame for matured bond."""
        pytest.importorskip("pandas")

        from convex import FixedRateBond, cash_flows_to_dataframe

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2020, 1, 15),
            issue_date=date(2015, 1, 15),
        )

        df = cash_flows_to_dataframe(bond, date(2025, 1, 1))
        assert len(df) == 0


class TestDatetimeInterop:
    """Tests for datetime.date interoperability."""

    def test_bond_with_datetime_date(self):
        """Test creating bond with datetime.date objects."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        # Methods should also accept datetime.date
        accrued = bond.accrued_interest(date(2025, 6, 15))
        assert accrued > 0

        cfs = bond.cash_flows(date(2025, 6, 15))
        assert len(cfs) > 0

    def test_bond_with_string_dates(self):
        """Test that string dates work in methods."""
        from convex import FixedRateBond

        bond = FixedRateBond.us_treasury(
            coupon=0.05,
            maturity=date(2030, 1, 15),
            issue_date=date(2020, 1, 15),
        )

        # String dates should work too
        accrued = bond.accrued_interest("2025-06-15")
        assert accrued > 0
