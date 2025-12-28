# Convex Python Bindings

Python bindings for the Convex fixed income analytics library, powered by PyO3.

## Installation

```bash
pip install convex
```

## Quick Start

```python
from datetime import date
from convex import FixedRateBond, Frequency, cash_flows_to_dataframe

# Create a US Treasury bond
bond = FixedRateBond.us_treasury(
    coupon=0.05,  # 5% coupon
    maturity=date(2030, 1, 15),
    issue_date=date(2020, 1, 15),
)

# Access properties
print(f"Coupon: {bond.coupon_rate:.2%}")  # 5.00%
print(f"Maturity: {bond.maturity}")        # 2030-01-15

# Calculate accrued interest
accrued = bond.accrued_interest(date(2025, 6, 15))
print(f"Accrued interest: ${accrued:.4f}")

# Get cash flows as a list
for cf in bond.cash_flows(date(2025, 6, 15)):
    print(f"{cf.date}: ${cf.amount:.2f} ({cf.flow_type})")

# Or convert to pandas DataFrame
df = cash_flows_to_dataframe(bond, date(2025, 6, 15))
print(df)
```

## Features

- **High Performance**: Core calculations in Rust for maximum speed
- **Pythonic API**: Feels natural to Python developers
- **pandas Integration**: Easy DataFrame conversion for analysis
- **Type Safety**: Strong typing with clear error messages

## Bond Types

- `FixedRateBond`: Standard fixed-rate coupon bonds
  - `FixedRateBond.us_corporate()`: US corporate bond conventions
  - `FixedRateBond.us_treasury()`: US Treasury bond conventions

## Coming Soon

- Yield calculations (YTM, YTC, YTW)
- Duration and convexity
- Spread calculations (Z-spread, OAS, I-spread, G-spread)
- Yield curve construction
- Zero coupon and floating rate bonds

## License

MIT License
