using System;
using System.Drawing;
using System.Windows.Forms;

namespace Convex.Excel
{
    /// <summary>
    /// Consolidated help form with tabs for different function categories.
    /// </summary>
    public class HelpForm : Form
    {
        private TabControl tabControl;
        private Button btnClose;

        public HelpForm()
        {
            InitializeComponent();
        }

        private void InitializeComponent()
        {
            this.Text = "Convex Help";
            this.Size = new Size(700, 550);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.MinimumSize = new Size(600, 400);
            this.FormBorderStyle = FormBorderStyle.Sizable;

            tabControl = new TabControl
            {
                Dock = DockStyle.Fill
            };

            // Curves tab
            var curvesTab = new TabPage("Curves");
            var curvesText = new RichTextBox
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                Font = new Font("Consolas", 9.5f),
                BackColor = Color.White,
                Text = @"CURVE FUNCTIONS
================

Creating Curves:
  =CX.CURVE(name, refDate, tenors, rates, interpolation, dayCount)

  Parameters:
    name          - Curve identifier (e.g., ""USD.GOVT"")
    refDate       - Reference/valuation date
    tenors        - Array of tenors in years (e.g., {1,2,5,10})
    rates         - Array of zero rates in % (e.g., {3.0,3.5,4.0,4.5})
    interpolation - 0=Linear, 1=LogLinear, 2=Cubic
    dayCount      - 0=ACT/360, 1=ACT/365, 2=ACT/ACT

Example:
  =CX.CURVE(""USD.GOVT"", TODAY(), {1,2,5,10,30}, {4.0,4.2,4.5,4.8,5.0}, 0, 1)

Querying Curves:
  =CX.CURVE.ZERO(handle, tenor)      - Zero rate at tenor
  =CX.CURVE.DISCOUNT(handle, tenor)  - Discount factor at tenor
  =CX.CURVE.FORWARD(handle, t1, t2)  - Forward rate from t1 to t2

Curve Manipulation:
  =CX.CURVE.SHIFT(handle, bps, name)           - Parallel shift
  =CX.CURVE.TWIST(handle, shortBp, longBp, pivot, name) - Twist/rotation
  =CX.CURVE.BUMP(handle, tenor, bps, name)     - Single tenor bump"
            };
            curvesTab.Controls.Add(curvesText);

            // Bonds tab
            var bondsTab = new TabPage("Bonds");
            var bondsText = new RichTextBox
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                Font = new Font("Consolas", 9.5f),
                BackColor = Color.White,
                Text = @"BOND FUNCTIONS
===============

Creating Bonds:
  =CX.BOND(isin, coupon%, freq, maturity, issue, dayCount, bdc)
  =CX.BOND.CORP(isin, coupon%, maturity, issue)     - US Corporate (30/360, semi)
  =CX.BOND.TSY(cusip, coupon%, maturity, issue)     - US Treasury (ACT/ACT, semi)
  =CX.BOND.CALLABLE(isin, coupon%, maturity, issue, callDate, callPrice, freq, dc)

Bond Queries:
  =CX.BOND.MATURITY(handle)      - Get maturity date
  =CX.BOND.COUPON(handle)        - Get coupon rate (%)
  =CX.BOND.ACCRUED(handle, settle) - Accrued interest
  =CX.BOND.CALL.DATE(handle)     - First call date (callable bonds)
  =CX.BOND.CALL.PRICE(handle)    - First call price (callable bonds)

Examples:
  =CX.BOND.CORP(""AAPL4.65%2026"", 4.65, DATE(2026,2,15), DATE(2021,2,15))
  =CX.BOND.TSY(""912828ZT5"", 1.5, DATE(2030,2,15), DATE(2020,2,18))

Day Count Conventions:
  0 = ACT/360
  1 = ACT/365
  2 = ACT/ACT ISDA
  3 = ACT/ACT ICMA
  4 = 30/360 US
  5 = 30E/360"
            };
            bondsTab.Controls.Add(bondsText);

            // Pricing tab
            var pricingTab = new TabPage("Pricing");
            var pricingText = new RichTextBox
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                Font = new Font("Consolas", 9.5f),
                BackColor = Color.White,
                Text = @"PRICING FUNCTIONS
==================

Yield Calculations:
  =CX.YIELD(bond, settle, cleanPrice, freq)     - Yield to maturity (%)
  =CX.YIELD.TRUE(bond, settle, cleanPrice, freq) - True yield (%)
  =CX.YIELD.ISMA(bond, settle, cleanPrice, freq) - ISMA yield (%)
  =CX.YIELD.CALL(bond, settle, cleanPrice)       - Yield to call (%)

Price Calculations:
  =CX.PRICE(bond, settle, yieldPct, freq)       - Clean price from yield
  =CX.DIRTY.PRICE(bond, settle, yieldPct, freq) - Dirty price from yield

Cashflows:
  =CX.CASHFLOWS(bond, settle)      - Array of all cashflow dates and amounts
  =CX.CASHFLOW.COUNT(bond, settle) - Number of remaining cashflows

Examples:
  =CX.YIELD(bondHandle, TODAY()+2, 99.50, 2)
  =CX.PRICE(bondHandle, TODAY()+2, 5.25, 2)

Yield Conventions:
  - Street Convention (default): Standard market convention
  - True Yield: Adjusted for actual/actual accrued
  - ISMA: International standard"
            };
            pricingTab.Controls.Add(pricingText);

            // Risk tab
            var riskTab = new TabPage("Risk Metrics");
            var riskText = new RichTextBox
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                Font = new Font("Consolas", 9.5f),
                BackColor = Color.White,
                Text = @"RISK METRICS FUNCTIONS
=======================

Duration:
  =CX.DURATION(bond, settle, cleanPrice, freq)     - Modified duration
  =CX.DURATION.MAC(bond, settle, cleanPrice, freq) - Macaulay duration

Convexity:
  =CX.CONVEXITY(bond, settle, cleanPrice, freq)    - Convexity

Dollar Value:
  =CX.DV01(bond, settle, cleanPrice, freq)         - Dollar value of 1bp

Comprehensive Analytics:
  =CX.ANALYTICS(bond, settle, cleanPrice, freq)
  Returns array: [CleanPrice, DirtyPrice, Accrued, YTM%, ModDur, MacDur, Cvx, DV01]

Interpretation:
  - Modified Duration: % price change per 1% yield change
  - Macaulay Duration: Weighted avg time to receive cashflows (years)
  - Convexity: Second-order price sensitivity
  - DV01: Dollar change per 1bp yield move (per $100 face)

Example:
  =CX.ANALYTICS(bondHandle, TODAY()+2, 101.50, 2)"
            };
            riskTab.Controls.Add(riskText);

            // Spreads tab
            var spreadsTab = new TabPage("Spreads");
            var spreadsText = new RichTextBox
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                Font = new Font("Consolas", 9.5f),
                BackColor = Color.White,
                Text = @"SPREAD FUNCTIONS
=================

Z-Spread (Zero-Volatility Spread):
  =CX.ZSPREAD(bond, curve, settle, cleanPrice)
  Constant spread over spot curve to match price

I-Spread (Interpolated Spread):
  =CX.ISPREAD(bond, swapCurve, settle, bondYield)
  Spread over interpolated swap rate at bond maturity

G-Spread (Government Spread):
  =CX.GSPREAD(bond, govtCurve, settle, bondYield)
  Spread over interpolated government bond yield

Asset Swap Spread:
  =CX.ASW(bond, swapCurve, settle, cleanPrice)
  Spread from par asset swap

Z-Spread with Analytics:
  =CX.ZSPREAD.ANALYTICS(bond, curve, settle, cleanPrice)
  Returns array: [Z-Spread (bps), Spread DV01, Spread Duration]

Examples:
  =CX.ZSPREAD(bondHandle, curveHandle, TODAY()+2, 98.50)
  =CX.GSPREAD(bondHandle, treasuryCurve, TODAY()+2, 5.25)"
            };
            spreadsTab.Controls.Add(spreadsText);

            // Tools tab
            var toolsTab = new TabPage("Tools");
            var toolsText = new RichTextBox
            {
                Dock = DockStyle.Fill,
                ReadOnly = true,
                Font = new Font("Consolas", 9.5f),
                BackColor = Color.White,
                Text = @"TOOLS & UTILITIES
==================

Object Management:
  =CX.LOOKUP(""name"")    - Find handle by name
  =CX.TYPE(handle)       - Get object type
  =CX.NAME(handle)       - Get object name
  =CX.COUNT()            - Count registered objects

Day Count Fraction:
  =CX.DAYFRAC(startDate, endDate, convention)

  Convention codes:
    0 = ACT/360
    1 = ACT/365
    2 = ACT/ACT ISDA
    3 = ACT/ACT ICMA
    4 = 30/360 US
    5 = 30E/360

Ribbon Tools:
  - Object Browser: View all registered objects
  - Curve Viewer: Visual curve analysis with chart
  - Bond Analyzer: Complete bond analytics
  - Clear All: Remove all registered objects

Tips:
  - Objects persist until cleared or Excel closes
  - Use descriptive names for easy lookup
  - Handles are displayed as CX-XXXX format"
            };
            toolsTab.Controls.Add(toolsText);

            tabControl.TabPages.Add(curvesTab);
            tabControl.TabPages.Add(bondsTab);
            tabControl.TabPages.Add(pricingTab);
            tabControl.TabPages.Add(riskTab);
            tabControl.TabPages.Add(spreadsTab);
            tabControl.TabPages.Add(toolsTab);

            // Button panel
            var buttonPanel = new Panel
            {
                Dock = DockStyle.Bottom,
                Height = 45,
                Padding = new Padding(5)
            };

            btnClose = new Button
            {
                Text = "Close",
                Width = 80,
                Anchor = AnchorStyles.Right | AnchorStyles.Bottom
            };
            btnClose.Location = new Point(buttonPanel.Width - btnClose.Width - 15, 8);
            btnClose.Click += (s, e) => this.Close();

            buttonPanel.Controls.Add(btnClose);

            this.Controls.Add(tabControl);
            this.Controls.Add(buttonPanel);

            this.Resize += (s, e) =>
            {
                btnClose.Location = new Point(buttonPanel.Width - btnClose.Width - 15, 8);
            };
        }
    }
}
