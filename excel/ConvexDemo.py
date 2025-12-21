"""
Generate Convex Excel Add-in Demo Workbook
Creates an Excel file showcasing all CX.* functions
Uses xlsxwriter to avoid Excel's implicit intersection @ operator
"""

import xlsxwriter
from datetime import date, timedelta

# Calculate dates
today = date.today()
maturity = today + timedelta(days=365*5)
issue = today - timedelta(days=365*2)
call_date = today + timedelta(days=365*2)

output_path = r"C:\Users\sujit\source\convex\excel\ConvexDemo.xlsx"

# Create workbook
wb = xlsxwriter.Workbook(output_path)

# Formats
title_fmt = wb.add_format({'bold': True, 'font_size': 14, 'font_color': 'white', 'bg_color': '#1F4E79'})
section_fmt = wb.add_format({'bold': True, 'font_size': 12, 'font_color': '#1F4E79'})
header_fmt = wb.add_format({'bold': True, 'font_size': 11, 'bg_color': '#D6DCE4', 'border': 1})
input_fmt = wb.add_format({'bg_color': '#FFF2CC', 'border': 1})
result_fmt = wb.add_format({'font_name': 'Consolas', 'font_size': 10, 'bg_color': '#E2EFDA', 'border': 1})
date_input_fmt = wb.add_format({'bg_color': '#FFF2CC', 'border': 1, 'num_format': 'yyyy-mm-dd'})
date_result_fmt = wb.add_format({'font_name': 'Consolas', 'bg_color': '#E2EFDA', 'border': 1, 'num_format': 'yyyy-mm-dd'})
cell_fmt = wb.add_format({'border': 1})

# ============================================================================
# Sheet 1: Overview
# ============================================================================
ws = wb.add_worksheet("Overview")
ws.set_column('A:A', 25)
ws.set_column('B:B', 50)

row = 0
ws.merge_range(row, 0, row, 6, "Convex Fixed Income Analytics - Excel Add-in Demo", title_fmt)
row += 2

ws.write(row, 0, "This workbook demonstrates all capabilities of the Convex Excel Add-in.")
row += 1
ws.write(row, 0, "Yellow cells = inputs you can modify. Green cells = formula results.")
row += 2

ws.write(row, 0, "Quick Start - Test Add-in Loading", section_fmt)
row += 1
ws.write(row, 0, "Version:")
ws.write_formula(row, 1, '=CX.VERSION()', result_fmt)
row += 1
ws.write(row, 0, "Load Status:")
ws.write_formula(row, 1, '=CX.LOAD.STATUS()', result_fmt)
row += 2

ws.write(row, 0, "Function Categories", section_fmt)
row += 1

ws.write(row, 0, "Category", header_fmt)
ws.write(row, 1, "Functions", header_fmt)
row += 1

categories = [
    ("Curves", "CX.CURVE, CX.CURVE.ZERO, CX.CURVE.DISCOUNT, CX.CURVE.FORWARD"),
    ("Bonds", "CX.BOND, CX.BOND.CORP, CX.BOND.TSY, CX.BOND.CALLABLE"),
    ("Pricing", "CX.YIELD, CX.PRICE, CX.DIRTY.PRICE, CX.BOND.ACCRUED"),
    ("Risk Metrics", "CX.DURATION, CX.DURATION.MAC, CX.CONVEXITY, CX.DV01"),
    ("Spreads", "CX.ZSPREAD, CX.ISPREAD, CX.GSPREAD, CX.ASW"),
    ("Bootstrapping", "CX.BOOTSTRAP, CX.BOOTSTRAP.OIS, CX.BOOTSTRAP.MIXED"),
]
for cat, funcs in categories:
    ws.write(row, 0, cat, cell_fmt)
    ws.write(row, 1, funcs, cell_fmt)
    row += 1

row += 1
ws.write(row, 0, "Note: All rate inputs/outputs are in percentage (e.g., 4.5 for 4.5%)")
row += 1
ws.write(row, 0, "Handles are returned as #CX#100, #CX#101, etc.")

# ============================================================================
# Sheet 2: Curves
# ============================================================================
ws = wb.add_worksheet("Curves")
ws.set_column('A:A', 20)
ws.set_column('B:B', 15)
ws.set_column('C:C', 15)
ws.set_column('D:D', 40)

row = 0
ws.merge_range(row, 0, row, 6, "Yield Curve Construction & Queries", title_fmt)
row += 2

ws.write(row, 0, "1. Input Data", section_fmt)
row += 1

ws.write(row, 0, "Reference Date:")
ws.write(row, 1, today, date_input_fmt)
ref_date_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Curve Name:")
ws.write(row, 1, "USD.GOVT", input_fmt)
name_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "Tenor (Years)", header_fmt)
ws.write(row, 1, "Zero Rate (%)", header_fmt)
row += 1

data_start = row + 1
tenors = [1, 2, 3, 5, 7, 10, 20, 30]
rates = [4.50, 4.25, 4.10, 4.00, 4.05, 4.15, 4.40, 4.50]
for t, r in zip(tenors, rates):
    ws.write(row, 0, t, input_fmt)
    ws.write(row, 1, r, input_fmt)
    row += 1
data_end = row

row += 1
ws.write(row, 0, "2. Create Curve", section_fmt)
row += 1
ws.write(row, 0, "Curve Handle:")
formula = f'=CX.CURVE({name_cell}, {ref_date_cell}, A{data_start}:A{data_end}, B{data_start}:B{data_end}, 0, 1)'
ws.write_formula(row, 1, formula, result_fmt)
curve_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Syntax:")
ws.write(row, 1, "CX.CURVE(name, refDate, tenors, rates, interp, daycount)")
row += 2

ws.write(row, 0, "3. Query Curve", section_fmt)
row += 1
ws.write(row, 0, "Query", header_fmt)
ws.write(row, 1, "Tenor", header_fmt)
ws.write(row, 2, "Result", header_fmt)
ws.write(row, 3, "Formula", header_fmt)
row += 1

queries = [
    ("Zero Rate @ 5Y", 5, f'=CX.CURVE.ZERO({curve_cell}, B{row+1})', 'CX.CURVE.ZERO(curve, tenor)'),
    ("Zero Rate @ 15Y", 15, f'=CX.CURVE.ZERO({curve_cell}, B{row+2})', 'CX.CURVE.ZERO(curve, tenor)'),
    ("Discount @ 5Y", 5, f'=CX.CURVE.DISCOUNT({curve_cell}, B{row+3})', 'CX.CURVE.DISCOUNT(curve, tenor)'),
    ("Discount @ 10Y", 10, f'=CX.CURVE.DISCOUNT({curve_cell}, B{row+4})', 'CX.CURVE.DISCOUNT(curve, tenor)'),
    ("Forward 2Y-5Y", 2, f'=CX.CURVE.FORWARD({curve_cell}, B{row+5}, 5)', 'CX.CURVE.FORWARD(curve, t1, t2)'),
    ("Forward 5Y-10Y", 5, f'=CX.CURVE.FORWARD({curve_cell}, B{row+6}, 10)', 'CX.CURVE.FORWARD(curve, t1, t2)'),
]
for label, tenor, formula, syntax in queries:
    ws.write(row, 0, label, cell_fmt)
    ws.write(row, 1, tenor, input_fmt)
    ws.write_formula(row, 2, formula, result_fmt)
    ws.write(row, 3, syntax, cell_fmt)
    row += 1

# ============================================================================
# Sheet 3: Bonds
# ============================================================================
ws = wb.add_worksheet("Bonds")
ws.set_column('A:A', 22)
ws.set_column('B:B', 20)
ws.set_column('C:C', 45)

row = 0
ws.merge_range(row, 0, row, 6, "Bond Creation", title_fmt)
row += 2

ws.write(row, 0, "1. Generic Fixed Rate Bond (CX.BOND)", section_fmt)
row += 1
ws.write(row, 0, "Syntax: CX.BOND(isin, coupon%, frequency, maturity, issue, daycount, bdc)")
row += 2

ws.write(row, 0, "ISIN:")
ws.write(row, 1, "US123456789", input_fmt)
isin_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Coupon (%):")
ws.write(row, 1, 5.0, input_fmt)
coupon_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Frequency:")
ws.write(row, 1, 2, input_fmt)
ws.write(row, 2, "(1=Annual, 2=Semi, 4=Quarterly)")
freq_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Maturity:")
ws.write(row, 1, maturity, date_input_fmt)
mat_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Issue Date:")
ws.write(row, 1, issue, date_input_fmt)
issue_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Day Count:")
ws.write(row, 1, 1, input_fmt)
ws.write(row, 2, "(0=Act/360, 1=30/360, 2=Act/365, 3=Act/Act)")
dc_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "Bond Handle:")
formula = f'=CX.BOND({isin_cell}, {coupon_cell}, {freq_cell}, {mat_cell}, {issue_cell}, {dc_cell}, 2)'
ws.write_formula(row, 1, formula, result_fmt)
row += 3

ws.write(row, 0, "2. US Corporate Bond (CX.BOND.CORP)", section_fmt)
row += 1
ws.write(row, 0, "Syntax: CX.BOND.CORP(isin, coupon%, maturity, issue)")
row += 1
ws.write(row, 0, "Corp Bond:")
formula = f'=CX.BOND.CORP("AAPL 5.0", 5.0, {mat_cell}, {issue_cell})'
ws.write_formula(row, 1, formula, result_fmt)
row += 3

ws.write(row, 0, "3. US Treasury Bond (CX.BOND.TSY)", section_fmt)
row += 1
ws.write(row, 0, "Syntax: CX.BOND.TSY(isin, coupon%, maturity, issue)")
row += 1
ws.write(row, 0, "Treasury:")
formula = f'=CX.BOND.TSY("T 4.5", 4.5, {mat_cell}, {issue_cell})'
ws.write_formula(row, 1, formula, result_fmt)
row += 3

ws.write(row, 0, "4. Callable Bond (CX.BOND.CALLABLE)", section_fmt)
row += 1
ws.write(row, 0, "Syntax: CX.BOND.CALLABLE(isin, coupon%, maturity, issue, callDate, callPrice)")
row += 1
ws.write(row, 0, "Call Date:")
ws.write(row, 1, call_date, date_input_fmt)
call_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Call Price:")
ws.write(row, 1, 100, input_fmt)
callpx_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Callable Bond:")
formula = f'=CX.BOND.CALLABLE("CALL 5.5", 5.5, {mat_cell}, {issue_cell}, {call_cell}, {callpx_cell})'
ws.write_formula(row, 1, formula, result_fmt)

# ============================================================================
# Sheet 4: Pricing
# ============================================================================
ws = wb.add_worksheet("Pricing")
ws.set_column('A:A', 25)
ws.set_column('B:B', 18)
ws.set_column('C:C', 45)

row = 0
ws.merge_range(row, 0, row, 6, "Bond Pricing & Yield Calculations", title_fmt)
row += 2

ws.write(row, 0, "Setup", section_fmt)
row += 1
ws.write(row, 0, "Bond Handle:")
formula = f'=CX.BOND.CORP("PRICING TEST", 5.0, DATE({maturity.year},{maturity.month},{maturity.day}), DATE({issue.year},{issue.month},{issue.day}))'
ws.write_formula(row, 1, formula, result_fmt)
bond_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Settlement Date:")
ws.write(row, 1, today, date_input_fmt)
settle_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "1. Yield from Price (CX.YIELD)", section_fmt)
row += 1
ws.write(row, 0, "Clean Price (input):")
ws.write(row, 1, 98.50, input_fmt)
price_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "YTM (%):")
formula = f'=CX.YIELD({bond_cell}, {settle_cell}, {price_cell})'
ws.write_formula(row, 1, formula, result_fmt)
row += 2

ws.write(row, 0, "2. Price from Yield (CX.PRICE)", section_fmt)
row += 1
ws.write(row, 0, "Yield (input %):")
ws.write(row, 1, 5.25, input_fmt)
yield_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Clean Price:")
formula = f'=CX.PRICE({bond_cell}, {settle_cell}, {yield_cell})'
ws.write_formula(row, 1, formula, result_fmt)
row += 2

ws.write(row, 0, "3. Dirty Price & Accrued", section_fmt)
row += 1
ws.write(row, 0, "Dirty Price:")
formula = f'=CX.DIRTY.PRICE({bond_cell}, {settle_cell}, {yield_cell})'
ws.write_formula(row, 1, formula, result_fmt)
row += 1
ws.write(row, 0, "Accrued Interest:")
formula = f'=CX.BOND.ACCRUED({bond_cell}, {settle_cell})'
ws.write_formula(row, 1, formula, result_fmt)

# ============================================================================
# Sheet 5: Risk
# ============================================================================
ws = wb.add_worksheet("Risk")
ws.set_column('A:A', 25)
ws.set_column('B:B', 18)
ws.set_column('C:C', 45)

row = 0
ws.merge_range(row, 0, row, 6, "Bond Risk Analytics", title_fmt)
row += 2

ws.write(row, 0, "Setup", section_fmt)
row += 1
ws.write(row, 0, "Bond Handle:")
formula = f'=CX.BOND.CORP("RISK TEST", 5.0, DATE({maturity.year},{maturity.month},{maturity.day}), DATE({issue.year},{issue.month},{issue.day}))'
ws.write_formula(row, 1, formula, result_fmt)
bond_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Settlement Date:")
ws.write(row, 1, today, date_input_fmt)
settle_cell = f"B{row+1}"
row += 1
ws.write(row, 0, "Yield (%):")
ws.write(row, 1, 5.0, input_fmt)
yield_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "Duration Measures", section_fmt)
row += 1
ws.write(row, 0, "Metric", header_fmt)
ws.write(row, 1, "Value", header_fmt)
ws.write(row, 2, "Formula", header_fmt)
row += 1
ws.write(row, 0, "Modified Duration", cell_fmt)
ws.write_formula(row, 1, f'=CX.DURATION({bond_cell}, {settle_cell}, {yield_cell})', result_fmt)
ws.write(row, 2, "CX.DURATION(bond, settle, yield%)", cell_fmt)
row += 1
ws.write(row, 0, "Macaulay Duration", cell_fmt)
ws.write_formula(row, 1, f'=CX.DURATION.MAC({bond_cell}, {settle_cell}, {yield_cell})', result_fmt)
ws.write(row, 2, "CX.DURATION.MAC(bond, settle, yield%)", cell_fmt)
row += 2

ws.write(row, 0, "Convexity & DV01", section_fmt)
row += 1
ws.write(row, 0, "Metric", header_fmt)
ws.write(row, 1, "Value", header_fmt)
ws.write(row, 2, "Formula", header_fmt)
row += 1
ws.write(row, 0, "Convexity", cell_fmt)
ws.write_formula(row, 1, f'=CX.CONVEXITY({bond_cell}, {settle_cell}, {yield_cell})', result_fmt)
ws.write(row, 2, "CX.CONVEXITY(bond, settle, yield%)", cell_fmt)
row += 1
ws.write(row, 0, "DV01 (per $100)", cell_fmt)
ws.write_formula(row, 1, f'=CX.DV01({bond_cell}, {settle_cell}, {yield_cell})', result_fmt)
ws.write(row, 2, "CX.DV01(bond, settle, yield%)", cell_fmt)

# ============================================================================
# Sheet 6: Spreads
# ============================================================================
ws = wb.add_worksheet("Spreads")
ws.set_column('A:A', 25)
ws.set_column('B:B', 18)
ws.set_column('C:C', 50)

row = 0
ws.merge_range(row, 0, row, 6, "Spread Calculations", title_fmt)
row += 2

ws.write(row, 0, "Setup: Create Bond and Curve", section_fmt)
row += 1
ws.write(row, 0, "Settlement Date:")
ws.write(row, 1, today, date_input_fmt)
settle_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "Bond Handle:")
formula = f'=CX.BOND.CORP("SPREAD TEST", 5.0, DATE({maturity.year},{maturity.month},{maturity.day}), DATE({issue.year},{issue.month},{issue.day}))'
ws.write_formula(row, 1, formula, result_fmt)
bond_cell = f"B{row+1}"
row += 1

ws.write(row, 0, "Govt Curve Handle:")
formula = f'=CX.CURVE("GOVT", {settle_cell}, {{1,2,5,10}}, {{4.0,4.1,4.2,4.3}}, 0, 1)'
ws.write_formula(row, 1, formula, result_fmt)
curve_cell = f"B{row+1}"
row += 1

ws.write(row, 0, "Clean Price:")
ws.write(row, 1, 98.50, input_fmt)
price_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "Spread Measures (bps)", section_fmt)
row += 1
ws.write(row, 0, "Spread", header_fmt)
ws.write(row, 1, "Value", header_fmt)
ws.write(row, 2, "Description", header_fmt)
row += 1

spreads = [
    ("Z-Spread", f'=CX.ZSPREAD({bond_cell}, {curve_cell}, {settle_cell}, {price_cell})', "Constant spread over zero curve"),
    ("I-Spread", f'=CX.ISPREAD({bond_cell}, {curve_cell}, {settle_cell}, {price_cell})', "Spread vs interpolated swap rate"),
    ("G-Spread", f'=CX.GSPREAD({bond_cell}, {curve_cell}, {settle_cell}, {price_cell})', "Spread vs government benchmark"),
    ("ASW Spread", f'=CX.ASW({bond_cell}, {curve_cell}, {settle_cell}, {price_cell})', "Asset swap spread"),
]
for name, formula, desc in spreads:
    ws.write(row, 0, name, cell_fmt)
    ws.write_formula(row, 1, formula, result_fmt)
    ws.write(row, 2, desc, cell_fmt)
    row += 1

# ============================================================================
# Sheet 7: Bootstrap
# ============================================================================
ws = wb.add_worksheet("Bootstrap")
ws.set_column('A:A', 22)
ws.set_column('B:B', 15)
ws.set_column('C:C', 15)
ws.set_column('D:D', 15)
ws.set_column('E:E', 15)

row = 0
ws.merge_range(row, 0, row, 6, "Curve Bootstrapping from Market Instruments", title_fmt)
row += 2

ws.write(row, 0, "1. Deposit & Swap Bootstrap", section_fmt)
row += 1
ws.write(row, 0, "Reference Date:")
ws.write(row, 1, today, date_input_fmt)
ref_cell = f"B{row+1}"
row += 2

ws.write(row, 0, "Deposits", header_fmt)
ws.write(row, 2, "Swaps", header_fmt)
row += 1
ws.write(row, 0, "Tenor", header_fmt)
ws.write(row, 1, "Rate (%)", header_fmt)
ws.write(row, 2, "Tenor", header_fmt)
ws.write(row, 3, "Rate (%)", header_fmt)
row += 1

deposit_data = [(0.25, 4.80), (0.5, 4.70), (1, 4.60)]
swap_data = [(2, 4.40), (3, 4.30), (5, 4.20), (7, 4.25), (10, 4.35)]

data_start = row + 1
max_rows = max(len(deposit_data), len(swap_data))
for i in range(max_rows):
    if i < len(deposit_data):
        ws.write(row, 0, deposit_data[i][0], input_fmt)
        ws.write(row, 1, deposit_data[i][1], input_fmt)
    if i < len(swap_data):
        ws.write(row, 2, swap_data[i][0], input_fmt)
        ws.write(row, 3, swap_data[i][1], input_fmt)
    row += 1
data_end = row
dep_end = data_start + len(deposit_data) - 1

row += 1
ws.write(row, 0, "Bootstrapped Curve:")
formula = f'=CX.BOOTSTRAP("USD.SWAP", {ref_cell}, A{data_start}:A{dep_end}, B{data_start}:B{dep_end}, C{data_start}:C{data_end}, D{data_start}:D{data_end}, 0, 1)'
ws.write_formula(row, 1, formula, result_fmt)
row += 3

ws.write(row, 0, "2. OIS Bootstrap", section_fmt)
row += 1
ws.write(row, 0, "Tenor", header_fmt)
ws.write(row, 1, "OIS Rate (%)", header_fmt)
row += 1

ois_start = row + 1
ois_data = [(0.25, 4.30), (0.5, 4.28), (1, 4.25), (2, 4.20), (5, 4.15)]
for t, r in ois_data:
    ws.write(row, 0, t, input_fmt)
    ws.write(row, 1, r, input_fmt)
    row += 1
ois_end = row

row += 1
ws.write(row, 0, "OIS Curve:")
formula = f'=CX.BOOTSTRAP.OIS("USD.OIS", {ref_cell}, A{ois_start}:A{ois_end}, B{ois_start}:B{ois_end}, 0, 1)'
ws.write_formula(row, 1, formula, result_fmt)
row += 3

ws.write(row, 0, "3. Mixed Instruments", section_fmt)
row += 1
ws.write(row, 0, "Types: 0=Deposit, 1=FRA, 2=Swap, 3=OIS")
row += 1
ws.write(row, 0, "Type", header_fmt)
ws.write(row, 1, "Tenor", header_fmt)
ws.write(row, 2, "Rate (%)", header_fmt)
row += 1

mixed_start = row + 1
mixed_data = [(0, 0.25, 4.80), (0, 0.5, 4.70), (2, 2, 4.40), (2, 5, 4.20), (2, 10, 4.35)]
for typ, t, r in mixed_data:
    ws.write(row, 0, typ, input_fmt)
    ws.write(row, 1, t, input_fmt)
    ws.write(row, 2, r, input_fmt)
    row += 1
mixed_end = row

row += 1
ws.write(row, 0, "Mixed Curve:")
formula = f'=CX.BOOTSTRAP.MIXED("USD.MIXED", {ref_cell}, A{mixed_start}:A{mixed_end}, B{mixed_start}:B{mixed_end}, C{mixed_start}:C{mixed_end}, 0, 1)'
ws.write_formula(row, 1, formula, result_fmt)

# ============================================================================
# Sheet 8: Reference
# ============================================================================
ws = wb.add_worksheet("Reference")
ws.set_column('A:A', 25)
ws.set_column('B:B', 18)
ws.set_column('C:C', 50)

row = 0
ws.merge_range(row, 0, row, 6, "Parameter Reference", title_fmt)
row += 2

ws.write(row, 0, "Interpolation Methods", section_fmt)
row += 1
ws.write(row, 0, "Code", header_fmt)
ws.write(row, 1, "Method", header_fmt)
ws.write(row, 2, "Description", header_fmt)
row += 1
for code, method, desc in [(0, "Linear", "Linear interpolation on zero rates"),
                            (1, "LogLinear", "Linear on log discount factors"),
                            (2, "Cubic", "Cubic spline"),
                            (3, "MonotoneConvex", "Hagan-West monotone convex")]:
    ws.write(row, 0, code, cell_fmt)
    ws.write(row, 1, method, cell_fmt)
    ws.write(row, 2, desc, cell_fmt)
    row += 1

row += 1
ws.write(row, 0, "Day Count Conventions", section_fmt)
row += 1
ws.write(row, 0, "Code", header_fmt)
ws.write(row, 1, "Convention", header_fmt)
ws.write(row, 2, "Description", header_fmt)
row += 1
for code, conv, desc in [(0, "Act/360", "Actual days / 360"),
                          (1, "30/360", "30 days per month / 360"),
                          (2, "Act/365", "Actual days / 365"),
                          (3, "Act/Act ISDA", "Actual / Actual (ISDA)"),
                          (4, "Act/Act ICMA", "Actual / Actual (ICMA)"),
                          (5, "Bus/252", "Business days / 252")]:
    ws.write(row, 0, code, cell_fmt)
    ws.write(row, 1, conv, cell_fmt)
    ws.write(row, 2, desc, cell_fmt)
    row += 1

row += 1
ws.write(row, 0, "Frequency Codes", section_fmt)
row += 1
ws.write(row, 0, "Code", header_fmt)
ws.write(row, 1, "Frequency", header_fmt)
ws.write(row, 2, "Payments/Year", header_fmt)
row += 1
for code, freq, ppy in [(1, "Annual", 1), (2, "Semi-Annual", 2), (4, "Quarterly", 4), (12, "Monthly", 12)]:
    ws.write(row, 0, code, cell_fmt)
    ws.write(row, 1, freq, cell_fmt)
    ws.write(row, 2, ppy, cell_fmt)
    row += 1

row += 1
ws.write(row, 0, "Instrument Types (Bootstrap)", section_fmt)
row += 1
ws.write(row, 0, "Code", header_fmt)
ws.write(row, 1, "Type", header_fmt)
ws.write(row, 2, "Description", header_fmt)
row += 1
for code, typ, desc in [(0, "Deposit", "Money market deposit"),
                         (1, "FRA", "Forward rate agreement"),
                         (2, "Swap", "Interest rate swap"),
                         (3, "OIS", "Overnight index swap")]:
    ws.write(row, 0, code, cell_fmt)
    ws.write(row, 1, typ, cell_fmt)
    ws.write(row, 2, desc, cell_fmt)
    row += 1

row += 1
ws.write(row, 0, "Business Day Conventions", section_fmt)
row += 1
ws.write(row, 0, "Code", header_fmt)
ws.write(row, 1, "Convention", header_fmt)
ws.write(row, 2, "Description", header_fmt)
row += 1
for code, conv, desc in [(0, "None", "No adjustment"),
                          (1, "Following", "Next business day"),
                          (2, "ModifiedFollowing", "Next, unless different month"),
                          (3, "Preceding", "Previous business day")]:
    ws.write(row, 0, code, cell_fmt)
    ws.write(row, 1, conv, cell_fmt)
    ws.write(row, 2, desc, cell_fmt)
    row += 1

wb.close()
print(f"Demo workbook created: {output_path}")
