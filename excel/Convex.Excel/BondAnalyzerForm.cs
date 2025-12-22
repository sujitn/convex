using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;
using System.Windows.Forms.DataVisualization.Charting;

namespace Convex.Excel
{
    /// <summary>
    /// Form to analyze bond details, calculate analytics, and view cashflows.
    /// </summary>
    public class BondAnalyzerForm : Form
    {
        private ComboBox cboBonds;
        private Button btnRefresh;
        private Label lblBondInfo;
        private DateTimePicker dtpSettlement;
        private NumericUpDown numPrice;
        private NumericUpDown numFrequency;
        private Button btnCalculate;
        private GroupBox grpDetails;
        private GroupBox grpAnalytics;
        private GroupBox grpCashflows;
        private DataGridView cashflowGrid;
        private Chart cashflowChart;
        private SplitContainer splitContainer;
        private Button btnClose;

        // Detail labels
        private Label lblMaturityValue;
        private Label lblCouponValue;
        private Label lblAccruedValue;

        // Analytics labels
        private Label lblYtmValue;
        private Label lblModDurValue;
        private Label lblMacDurValue;
        private Label lblConvexityValue;
        private Label lblDv01Value;
        private Label lblDirtyPriceValue;

        // Callable bond analytics
        private GroupBox grpCallableAnalytics;
        private Label lblYtwValue;
        private Label lblOasValue;
        private Label lblEffDurValue;
        private Label lblEffConvValue;
        private Label lblWorkoutDateValue;
        private NumericUpDown numVolatility;
        private ComboBox cboCurve;

        // FRN analytics
        private GroupBox grpFrnAnalytics;
        private Label lblDiscountMarginValue;
        private Label lblSimpleMarginValue;
        private NumericUpDown numCurrentIndex;

        public BondAnalyzerForm()
        {
            InitializeComponent();
            RefreshBondList();
        }

        private void InitializeComponent()
        {
            this.Text = "Bond Analyzer";
            this.Size = new Size(950, 700);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.MinimumSize = new Size(800, 600);
            this.FormBorderStyle = FormBorderStyle.Sizable;

            // Top panel for bond selection
            var topPanel = new Panel
            {
                Dock = DockStyle.Top,
                Height = 45,
                Padding = new Padding(5)
            };

            var lblBond = new Label
            {
                Text = "Bond:",
                Location = new Point(10, 14),
                AutoSize = true
            };

            cboBonds = new ComboBox
            {
                Location = new Point(50, 10),
                Width = 280,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboBonds.SelectedIndexChanged += CboBonds_SelectedIndexChanged;

            btnRefresh = new Button
            {
                Text = "Refresh",
                Location = new Point(340, 9),
                Width = 70
            };
            btnRefresh.Click += (s, e) => RefreshBondList();

            lblBondInfo = new Label
            {
                Location = new Point(420, 14),
                AutoSize = true,
                Text = ""
            };

            topPanel.Controls.Add(lblBond);
            topPanel.Controls.Add(cboBonds);
            topPanel.Controls.Add(btnRefresh);
            topPanel.Controls.Add(lblBondInfo);

            // Input panel for settlement and price
            var inputPanel = new Panel
            {
                Dock = DockStyle.Top,
                Height = 220
            };

            var lblSettle = new Label
            {
                Text = "Settlement:",
                Location = new Point(10, 12),
                AutoSize = true
            };

            dtpSettlement = new DateTimePicker
            {
                Location = new Point(80, 9),
                Width = 110,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today
            };

            var lblPrice = new Label
            {
                Text = "Price:",
                Location = new Point(200, 12),
                AutoSize = true
            };

            numPrice = new NumericUpDown
            {
                Location = new Point(240, 9),
                Width = 70,
                DecimalPlaces = 3,
                Minimum = 0,
                Maximum = 200,
                Value = 100,
                Increment = 0.25m
            };

            var lblFreq = new Label
            {
                Text = "Freq:",
                Location = new Point(320, 12),
                AutoSize = true
            };

            numFrequency = new NumericUpDown
            {
                Location = new Point(365, 9),
                Width = 45,
                Minimum = 1,
                Maximum = 12,
                Value = 2
            };

            btnCalculate = new Button
            {
                Text = "Calculate",
                Location = new Point(430, 7),
                Width = 75
            };
            btnCalculate.Click += BtnCalculate_Click;

            // Bond details group
            grpDetails = new GroupBox
            {
                Text = "Bond Details",
                Location = new Point(10, 38),
                Size = new Size(480, 42)
            };

            var lblMaturity = new Label { Text = "Maturity:", Location = new Point(10, 18), AutoSize = true };
            lblMaturityValue = new Label { Text = "-", Location = new Point(65, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblCoupon = new Label { Text = "Coupon:", Location = new Point(170, 18), AutoSize = true };
            lblCouponValue = new Label { Text = "-", Location = new Point(225, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblAccrued = new Label { Text = "Accrued:", Location = new Point(320, 18), AutoSize = true };
            lblAccruedValue = new Label { Text = "-", Location = new Point(375, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };

            grpDetails.Controls.AddRange(new Control[] { lblMaturity, lblMaturityValue, lblCoupon, lblCouponValue, lblAccrued, lblAccruedValue });

            // Analytics group
            grpAnalytics = new GroupBox
            {
                Text = "Analytics",
                Location = new Point(10, 82),
                Size = new Size(900, 42)
            };

            var lblYtm = new Label { Text = "YTM:", Location = new Point(10, 18), AutoSize = true };
            lblYtmValue = new Label { Text = "-", Location = new Point(45, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold), ForeColor = Color.DarkBlue };
            var lblModDur = new Label { Text = "ModDur:", Location = new Point(120, 18), AutoSize = true };
            lblModDurValue = new Label { Text = "-", Location = new Point(175, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblMacDur = new Label { Text = "MacDur:", Location = new Point(250, 18), AutoSize = true };
            lblMacDurValue = new Label { Text = "-", Location = new Point(305, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblConvexity = new Label { Text = "Convexity:", Location = new Point(380, 18), AutoSize = true };
            lblConvexityValue = new Label { Text = "-", Location = new Point(445, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblDv01 = new Label { Text = "DV01:", Location = new Point(520, 18), AutoSize = true };
            lblDv01Value = new Label { Text = "-", Location = new Point(560, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold), ForeColor = Color.DarkRed };
            var lblDirty = new Label { Text = "Dirty:", Location = new Point(640, 18), AutoSize = true };
            lblDirtyPriceValue = new Label { Text = "-", Location = new Point(680, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };

            grpAnalytics.Controls.AddRange(new Control[] {
                lblYtm, lblYtmValue, lblModDur, lblModDurValue, lblMacDur, lblMacDurValue,
                lblConvexity, lblConvexityValue, lblDv01, lblDv01Value, lblDirty, lblDirtyPriceValue
            });

            // Callable bond analytics group
            grpCallableAnalytics = new GroupBox
            {
                Text = "Callable Bond Analytics",
                Location = new Point(10, 126),
                Size = new Size(900, 42),
                Visible = false
            };

            var lblYtw = new Label { Text = "YTW:", Location = new Point(10, 18), AutoSize = true };
            lblYtwValue = new Label { Text = "-", Location = new Point(50, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold), ForeColor = Color.DarkGreen };
            var lblWorkout = new Label { Text = "Workout:", Location = new Point(120, 18), AutoSize = true };
            lblWorkoutDateValue = new Label { Text = "-", Location = new Point(180, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblOas = new Label { Text = "OAS:", Location = new Point(280, 18), AutoSize = true };
            lblOasValue = new Label { Text = "-", Location = new Point(315, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold), ForeColor = Color.Purple };
            var lblEffDur = new Label { Text = "Eff Dur:", Location = new Point(400, 18), AutoSize = true };
            lblEffDurValue = new Label { Text = "-", Location = new Point(455, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblEffConv = new Label { Text = "Eff Conv:", Location = new Point(525, 18), AutoSize = true };
            lblEffConvValue = new Label { Text = "-", Location = new Point(590, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblVol = new Label { Text = "Vol%:", Location = new Point(665, 18), AutoSize = true };
            numVolatility = new NumericUpDown
            {
                Location = new Point(705, 15),
                Width = 50,
                DecimalPlaces = 1,
                Minimum = 0.1m,
                Maximum = 50,
                Value = 1.0m,
                Increment = 0.1m
            };
            var lblCurveLabel = new Label { Text = "Curve:", Location = new Point(765, 18), AutoSize = true };
            cboCurve = new ComboBox
            {
                Location = new Point(810, 15),
                Width = 80,
                DropDownStyle = ComboBoxStyle.DropDownList
            };

            grpCallableAnalytics.Controls.AddRange(new Control[] {
                lblYtw, lblYtwValue, lblWorkout, lblWorkoutDateValue,
                lblOas, lblOasValue, lblEffDur, lblEffDurValue, lblEffConv, lblEffConvValue,
                lblVol, numVolatility, lblCurveLabel, cboCurve
            });

            // FRN analytics group
            grpFrnAnalytics = new GroupBox
            {
                Text = "FRN Analytics",
                Location = new Point(10, 170),
                Size = new Size(500, 42),
                Visible = false
            };

            var lblDM = new Label { Text = "Disc Margin:", Location = new Point(10, 18), AutoSize = true };
            lblDiscountMarginValue = new Label { Text = "-", Location = new Point(90, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold), ForeColor = Color.DarkBlue };
            var lblSM = new Label { Text = "Simple Margin:", Location = new Point(180, 18), AutoSize = true };
            lblSimpleMarginValue = new Label { Text = "-", Location = new Point(275, 18), AutoSize = true, Font = new Font(this.Font, FontStyle.Bold) };
            var lblCurIdx = new Label { Text = "Index Rate%:", Location = new Point(355, 18), AutoSize = true };
            numCurrentIndex = new NumericUpDown
            {
                Location = new Point(435, 15),
                Width = 55,
                DecimalPlaces = 2,
                Minimum = 0,
                Maximum = 20,
                Value = 5.0m,
                Increment = 0.05m
            };

            grpFrnAnalytics.Controls.AddRange(new Control[] {
                lblDM, lblDiscountMarginValue, lblSM, lblSimpleMarginValue, lblCurIdx, numCurrentIndex
            });

            inputPanel.Controls.AddRange(new Control[] {
                lblSettle, dtpSettlement, lblPrice, numPrice, lblFreq, numFrequency, btnCalculate,
                grpDetails, grpAnalytics, grpCallableAnalytics, grpFrnAnalytics
            });

            // Split container for chart and cashflow grid
            splitContainer = new SplitContainer
            {
                Dock = DockStyle.Fill,
                Orientation = Orientation.Horizontal,
                BorderStyle = BorderStyle.None
            };

            // Cashflow chart
            cashflowChart = new Chart
            {
                Dock = DockStyle.Fill,
                BackColor = Color.White
            };

            var chartArea = new ChartArea("MainArea")
            {
                BackColor = Color.White
            };
            chartArea.AxisX.Title = "Payment Date";
            chartArea.AxisX.TitleFont = new Font("Segoe UI", 9f, FontStyle.Bold);
            chartArea.AxisX.MajorGrid.LineColor = Color.LightGray;
            chartArea.AxisX.LabelStyle.Angle = -45;
            chartArea.AxisY.Title = "Amount";
            chartArea.AxisY.TitleFont = new Font("Segoe UI", 9f, FontStyle.Bold);
            chartArea.AxisY.MajorGrid.LineColor = Color.LightGray;
            cashflowChart.ChartAreas.Add(chartArea);

            var cashflowSeries = new Series("Cashflows")
            {
                ChartType = SeriesChartType.Column,
                Color = Color.SteelBlue
            };
            cashflowChart.Series.Add(cashflowSeries);

            var legend = new Legend
            {
                Docking = Docking.Top,
                Alignment = StringAlignment.Center
            };
            cashflowChart.Legends.Add(legend);

            splitContainer.Panel1.Controls.Add(cashflowChart);

            // Cashflow grid
            grpCashflows = new GroupBox
            {
                Text = "Cashflow Schedule",
                Dock = DockStyle.Fill,
                Padding = new Padding(5)
            };

            cashflowGrid = new DataGridView
            {
                Dock = DockStyle.Fill,
                AllowUserToAddRows = false,
                AllowUserToDeleteRows = false,
                ReadOnly = true,
                SelectionMode = DataGridViewSelectionMode.FullRowSelect,
                MultiSelect = false,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                RowHeadersVisible = false,
                BackgroundColor = SystemColors.Window,
                BorderStyle = BorderStyle.Fixed3D
            };

            cashflowGrid.Columns.Add("Date", "Date");
            cashflowGrid.Columns.Add("Amount", "Amount");
            cashflowGrid.Columns.Add("Type", "Type");
            cashflowGrid.Columns.Add("DF", "Discount Factor");
            cashflowGrid.Columns.Add("PV", "Present Value");

            grpCashflows.Controls.Add(cashflowGrid);
            splitContainer.Panel2.Controls.Add(grpCashflows);

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

            // Add controls in correct order
            this.Controls.Add(splitContainer);
            this.Controls.Add(inputPanel);
            this.Controls.Add(topPanel);
            this.Controls.Add(buttonPanel);

            // Handle resize
            this.Resize += (s, e) =>
            {
                btnClose.Location = new Point(buttonPanel.Width - btnClose.Width - 15, 8);
            };
        }

        private void RefreshBondList()
        {
            cboBonds.Items.Clear();
            var bonds = new List<BondInfo>();

            // Enumerate bonds from registry (types 2-5 are bond types)
            NativeMethods.ObjectEnumCallback callback = (handle, objType, namePtr) =>
            {
                // 2=FixedBond, 3=ZeroBond, 4=FRN, 5=CallableBond
                if (objType >= 2 && objType <= 5)
                {
                    string name = namePtr != IntPtr.Zero
                        ? System.Runtime.InteropServices.Marshal.PtrToStringAnsi(namePtr)
                        : "";
                    string typeStr = objType switch
                    {
                        2 => "Fixed",
                        3 => "Zero",
                        4 => "FRN",
                        5 => "Callable",
                        _ => "Bond"
                    };
                    bonds.Add(new BondInfo { Handle = handle, Name = name, TypeString = typeStr });
                }
            };

            // Enumerate all objects (filter type 0 = all)
            NativeMethods.convex_enumerate_objects(callback, 0);

            foreach (var bond in bonds)
            {
                string displayName = string.IsNullOrEmpty(bond.Name)
                    ? $"[{bond.TypeString}] {HandleHelper.Format(bond.Handle)}"
                    : $"[{bond.TypeString}] {bond.Name} ({HandleHelper.Format(bond.Handle)})";
                cboBonds.Items.Add(new ComboBoxItem { Text = displayName, Value = bond.Handle });
            }

            if (cboBonds.Items.Count > 0)
                cboBonds.SelectedIndex = 0;
            else
                ClearDisplay();

            GC.KeepAlive(callback);
        }

        private void CboBonds_SelectedIndexChanged(object sender, EventArgs e)
        {
            if (cboBonds.SelectedItem is ComboBoxItem item)
            {
                LoadBondDetails(item.Value);
            }
        }

        private void LoadBondDetails(ulong handle)
        {
            // Get object type to determine which analytics to show
            var objType = ConvexWrapper.GetObjectType(handle);
            bool isCallable = objType == ConvexWrapper.ObjectType.CallableBond;
            bool isFRN = objType == ConvexWrapper.ObjectType.FloatingRateNote;

            // Show/hide appropriate analytics groups
            grpCallableAnalytics.Visible = isCallable;
            grpFrnAnalytics.Visible = isFRN;

            // Refresh curve dropdown for OAS/DM if needed
            if (isCallable || isFRN)
            {
                RefreshCurveList();
            }

            // Get maturity date
            int maturityInt = NativeMethods.convex_bond_maturity(handle);
            if (maturityInt > 0)
            {
                int year = maturityInt / 10000;
                int month = (maturityInt / 100) % 100;
                int day = maturityInt % 100;
                lblMaturityValue.Text = $"{year}-{month:D2}-{day:D2}";
            }
            else
            {
                lblMaturityValue.Text = "-";
            }

            // Get coupon rate
            double coupon = NativeMethods.convex_bond_coupon_rate(handle);
            lblCouponValue.Text = double.IsNaN(coupon) ? "-" : $"{coupon * 100:F3}%";

            // Calculate accrued as of settlement
            DateTime settle = dtpSettlement.Value;
            double accrued = NativeMethods.convex_bond_accrued(handle, settle.Year, settle.Month, settle.Day);
            lblAccruedValue.Text = double.IsNaN(accrued) ? "-" : accrued.ToString("F4");

            lblBondInfo.Text = $"Handle: {HandleHelper.Format(handle)}";

            // Clear callable/FRN analytics
            ClearCallableAnalytics();
            ClearFrnAnalytics();
        }

        private void RefreshCurveList()
        {
            cboCurve.Items.Clear();
            var curves = new List<CurveInfo>();

            NativeMethods.ObjectEnumCallback callback = (handle, objType, namePtr) =>
            {
                if (objType == 1) // Curve
                {
                    string name = namePtr != IntPtr.Zero
                        ? System.Runtime.InteropServices.Marshal.PtrToStringAnsi(namePtr)
                        : "";
                    curves.Add(new CurveInfo { Handle = handle, Name = name });
                }
            };

            NativeMethods.convex_enumerate_objects(callback, 1);

            foreach (var curve in curves)
            {
                string displayName = string.IsNullOrEmpty(curve.Name)
                    ? HandleHelper.Format(curve.Handle)
                    : $"{curve.Name} ({HandleHelper.Format(curve.Handle)})";
                cboCurve.Items.Add(new ComboBoxItem { Text = displayName, Value = curve.Handle });
            }

            if (cboCurve.Items.Count > 0)
                cboCurve.SelectedIndex = 0;

            GC.KeepAlive(callback);
        }

        private void ClearCallableAnalytics()
        {
            lblYtwValue.Text = "-";
            lblWorkoutDateValue.Text = "-";
            lblOasValue.Text = "-";
            lblEffDurValue.Text = "-";
            lblEffConvValue.Text = "-";
        }

        private void ClearFrnAnalytics()
        {
            lblDiscountMarginValue.Text = "-";
            lblSimpleMarginValue.Text = "-";
        }

        private class CurveInfo
        {
            public ulong Handle { get; set; }
            public string Name { get; set; }
        }

        private void BtnCalculate_Click(object sender, EventArgs e)
        {
            if (cboBonds.SelectedItem is ComboBoxItem item)
            {
                CalculateAnalytics(item.Value);
            }
        }

        private void CalculateAnalytics(ulong handle)
        {
            DateTime settle = dtpSettlement.Value;
            double cleanPrice = (double)numPrice.Value;
            int frequency = (int)numFrequency.Value;

            // Calculate comprehensive analytics
            int result = NativeMethods.convex_bond_analytics(
                handle,
                settle.Year, settle.Month, settle.Day,
                cleanPrice,
                frequency,
                out NativeMethods.FfiBondAnalytics analytics);

            if (result == NativeMethods.CONVEX_OK)
            {
                lblYtmValue.Text = $"{analytics.YieldToMaturity * 100:F4}%";
                lblModDurValue.Text = analytics.ModifiedDuration.ToString("F4");
                lblMacDurValue.Text = analytics.MacaulayDuration.ToString("F4");
                lblConvexityValue.Text = analytics.Convexity.ToString("F4");
                lblDv01Value.Text = analytics.Dv01.ToString("F4");
                lblDirtyPriceValue.Text = analytics.DirtyPrice.ToString("F4");
                lblAccruedValue.Text = analytics.Accrued.ToString("F4");
            }
            else
            {
                lblYtmValue.Text = "Error";
                lblModDurValue.Text = "-";
                lblMacDurValue.Text = "-";
                lblConvexityValue.Text = "-";
                lblDv01Value.Text = "-";
                lblDirtyPriceValue.Text = "-";
            }

            // Determine bond type and calculate specialized analytics
            var objType = ConvexWrapper.GetObjectType(handle);

            if (objType == ConvexWrapper.ObjectType.CallableBond)
            {
                CalculateCallableAnalytics(handle, settle, cleanPrice, analytics.DirtyPrice);
            }
            else if (objType == ConvexWrapper.ObjectType.FloatingRateNote)
            {
                CalculateFrnAnalytics(handle, settle, analytics.DirtyPrice);
            }

            // Load cashflows
            LoadCashflows(handle, settle);
        }

        private void CalculateCallableAnalytics(ulong handle, DateTime settle, double cleanPrice, double dirtyPrice)
        {
            // Calculate Yield to Worst
            var ytwResult = ConvexWrapper.CalculateYieldToWorst(handle, settle, cleanPrice);
            if (ytwResult != null)
            {
                lblYtwValue.Text = $"{ytwResult.Yield * 100:F4}%";
                lblWorkoutDateValue.Text = ytwResult.WorkoutDate.ToString("yyyy-MM-dd");
            }
            else
            {
                lblYtwValue.Text = "Error";
                lblWorkoutDateValue.Text = "-";
            }

            // Calculate OAS if a curve is selected
            if (cboCurve.SelectedItem is ComboBoxItem curveItem)
            {
                double volatility = (double)numVolatility.Value / 100.0; // Convert from % to decimal
                var oasResult = ConvexWrapper.CalculateOASAnalytics(
                    handle, curveItem.Value, settle, dirtyPrice, volatility);

                if (oasResult != null)
                {
                    lblOasValue.Text = $"{oasResult.OasBps:F2} bps";
                    lblEffDurValue.Text = oasResult.EffectiveDuration.ToString("F4");
                    lblEffConvValue.Text = oasResult.EffectiveConvexity.ToString("F4");
                }
                else
                {
                    lblOasValue.Text = "N/A";
                    lblEffDurValue.Text = "-";
                    lblEffConvValue.Text = "-";
                }
            }
            else
            {
                lblOasValue.Text = "No curve";
                lblEffDurValue.Text = "-";
                lblEffConvValue.Text = "-";
            }
        }

        private void CalculateFrnAnalytics(ulong handle, DateTime settle, double dirtyPrice)
        {
            // Calculate Simple Margin
            double currentIndex = (double)numCurrentIndex.Value / 100.0; // Convert from % to decimal
            double simpleMargin = ConvexWrapper.CalculateSimpleMargin(handle, settle, dirtyPrice, currentIndex);
            if (!double.IsNaN(simpleMargin))
            {
                lblSimpleMarginValue.Text = $"{simpleMargin:F2} bps";
            }
            else
            {
                lblSimpleMarginValue.Text = "Error";
            }

            // Calculate Discount Margin if a curve is selected
            if (cboCurve.SelectedItem is ComboBoxItem curveItem)
            {
                // Use the same curve for forward and discount (simplified)
                double dm = ConvexWrapper.CalculateDiscountMargin(
                    handle, curveItem.Value, curveItem.Value, settle, dirtyPrice);
                if (!double.IsNaN(dm))
                {
                    lblDiscountMarginValue.Text = $"{dm:F2} bps";
                }
                else
                {
                    lblDiscountMarginValue.Text = "Error";
                }
            }
            else
            {
                lblDiscountMarginValue.Text = "No curve";
            }
        }

        private void LoadCashflows(ulong handle, DateTime settle)
        {
            cashflowGrid.Rows.Clear();
            cashflowChart.Series["Cashflows"].Points.Clear();

            int count = NativeMethods.convex_bond_cashflow_count(handle, settle.Year, settle.Month, settle.Day);
            if (count <= 0)
                return;

            double totalPV = 0;
            for (int i = 0; i < count; i++)
            {
                int cfResult = NativeMethods.convex_bond_cashflow_get(
                    handle,
                    settle.Year, settle.Month, settle.Day,
                    i,
                    out int dateInt,
                    out double amount);

                if (cfResult == NativeMethods.CONVEX_OK && dateInt > 0)
                {
                    int year = dateInt / 10000;
                    int month = (dateInt / 100) % 100;
                    int day = dateInt % 100;
                    string dateStr = $"{year}-{month:D2}-{day:D2}";

                    // Determine if coupon or principal
                    string cfType = (amount > 50) ? "Principal + Coupon" : "Coupon";
                    if (i == count - 1 && amount > 50)
                        cfType = "Principal + Coupon";

                    // Estimate DF (simplified - would need curve for accurate)
                    double yearsToPayment = (new DateTime(year, month, day) - settle).TotalDays / 365.25;
                    double df = 1.0; // Placeholder

                    double pv = amount * df;
                    totalPV += pv;

                    cashflowGrid.Rows.Add(dateStr, amount.ToString("F4"), cfType, df.ToString("F6"), pv.ToString("F4"));

                    // Add to chart
                    var point = cashflowChart.Series["Cashflows"].Points.AddXY(dateStr, amount);
                    if (amount > 50)
                        cashflowChart.Series["Cashflows"].Points[point].Color = Color.DarkOrange;
                }
            }
        }

        private void ClearDisplay()
        {
            lblBondInfo.Text = "No bonds";
            lblMaturityValue.Text = "-";
            lblCouponValue.Text = "-";
            lblAccruedValue.Text = "-";
            lblYtmValue.Text = "-";
            lblModDurValue.Text = "-";
            lblMacDurValue.Text = "-";
            lblConvexityValue.Text = "-";
            lblDv01Value.Text = "-";
            lblDirtyPriceValue.Text = "-";
            ClearCallableAnalytics();
            ClearFrnAnalytics();
            grpCallableAnalytics.Visible = false;
            grpFrnAnalytics.Visible = false;
            cashflowGrid.Rows.Clear();
            cashflowChart.Series["Cashflows"].Points.Clear();
        }

        private class BondInfo
        {
            public ulong Handle { get; set; }
            public string Name { get; set; }
            public string TypeString { get; set; }
        }

        private class ComboBoxItem
        {
            public string Text { get; set; }
            public ulong Value { get; set; }
            public override string ToString() => Text;
        }
    }
}
