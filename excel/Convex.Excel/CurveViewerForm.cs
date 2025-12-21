using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;
using System.Windows.Forms.DataVisualization.Charting;

namespace Convex.Excel
{
    /// <summary>
    /// Form to view curve data points, chart, and query curve values.
    /// </summary>
    public class CurveViewerForm : Form
    {
        private ComboBox cboCurves;
        private DataGridView dataGrid;
        private Chart curveChart;
        private Button btnRefresh;
        private Button btnClose;
        private Label lblCurveInfo;
        private GroupBox grpQuery;
        private NumericUpDown numTenor;
        private Button btnQuery;
        private Label lblZeroRate;
        private Label lblDF;
        private Label lblZeroRateValue;
        private Label lblDFValue;
        private SplitContainer splitContainer;

        public CurveViewerForm()
        {
            InitializeComponent();
            RefreshCurveList();
        }

        private void InitializeComponent()
        {
            this.Text = "Curve Viewer";
            this.Size = new Size(900, 600);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.MinimumSize = new Size(700, 500);
            this.FormBorderStyle = FormBorderStyle.Sizable;

            // Top panel for curve selection
            var topPanel = new Panel
            {
                Dock = DockStyle.Top,
                Height = 45,
                Padding = new Padding(5)
            };

            var lblCurve = new Label
            {
                Text = "Curve:",
                Location = new Point(10, 14),
                AutoSize = true
            };

            cboCurves = new ComboBox
            {
                Location = new Point(55, 10),
                Width = 250,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboCurves.SelectedIndexChanged += CboCurves_SelectedIndexChanged;

            btnRefresh = new Button
            {
                Text = "Refresh",
                Location = new Point(320, 9),
                Width = 70
            };
            btnRefresh.Click += (s, e) => RefreshCurveList();

            lblCurveInfo = new Label
            {
                Location = new Point(400, 14),
                AutoSize = true,
                Text = ""
            };

            topPanel.Controls.Add(lblCurve);
            topPanel.Controls.Add(cboCurves);
            topPanel.Controls.Add(btnRefresh);
            topPanel.Controls.Add(lblCurveInfo);

            // Query panel
            grpQuery = new GroupBox
            {
                Text = "Query Curve",
                Dock = DockStyle.Top,
                Height = 70,
                Padding = new Padding(5)
            };

            var lblTenor = new Label
            {
                Text = "Tenor (years):",
                Location = new Point(10, 25),
                AutoSize = true
            };

            numTenor = new NumericUpDown
            {
                Location = new Point(95, 22),
                Width = 80,
                DecimalPlaces = 2,
                Minimum = 0,
                Maximum = 50,
                Value = 5,
                Increment = 0.25m
            };

            btnQuery = new Button
            {
                Text = "Query",
                Location = new Point(185, 21),
                Width = 60
            };
            btnQuery.Click += BtnQuery_Click;

            lblZeroRate = new Label
            {
                Text = "Zero Rate:",
                Location = new Point(260, 25),
                AutoSize = true
            };

            lblZeroRateValue = new Label
            {
                Text = "-",
                Location = new Point(330, 25),
                AutoSize = true,
                Font = new Font(this.Font, FontStyle.Bold)
            };

            lblDF = new Label
            {
                Text = "DF:",
                Location = new Point(420, 25),
                AutoSize = true
            };

            lblDFValue = new Label
            {
                Text = "-",
                Location = new Point(450, 25),
                AutoSize = true,
                Font = new Font(this.Font, FontStyle.Bold)
            };

            grpQuery.Controls.Add(lblTenor);
            grpQuery.Controls.Add(numTenor);
            grpQuery.Controls.Add(btnQuery);
            grpQuery.Controls.Add(lblZeroRate);
            grpQuery.Controls.Add(lblZeroRateValue);
            grpQuery.Controls.Add(lblDF);
            grpQuery.Controls.Add(lblDFValue);

            // Split container for chart and grid
            splitContainer = new SplitContainer
            {
                Dock = DockStyle.Fill,
                Orientation = Orientation.Vertical,
                BorderStyle = BorderStyle.None
            };

            // Chart for curve visualization
            curveChart = new Chart
            {
                Dock = DockStyle.Fill,
                BackColor = Color.White
            };

            var chartArea = new ChartArea("MainArea")
            {
                BackColor = Color.White
            };
            chartArea.AxisX.Title = "Tenor (Years)";
            chartArea.AxisX.TitleFont = new Font("Segoe UI", 9f, FontStyle.Bold);
            chartArea.AxisX.MajorGrid.LineColor = Color.LightGray;
            chartArea.AxisX.Minimum = 0;
            chartArea.AxisY.Title = "Zero Rate (%)";
            chartArea.AxisY.TitleFont = new Font("Segoe UI", 9f, FontStyle.Bold);
            chartArea.AxisY.MajorGrid.LineColor = Color.LightGray;
            chartArea.AxisY.LabelStyle.Format = "F2";
            curveChart.ChartAreas.Add(chartArea);

            var zeroRateSeries = new Series("Zero Rate")
            {
                ChartType = SeriesChartType.Line,
                BorderWidth = 2,
                Color = Color.RoyalBlue,
                MarkerStyle = MarkerStyle.Circle,
                MarkerSize = 6,
                MarkerColor = Color.RoyalBlue
            };
            curveChart.Series.Add(zeroRateSeries);

            var fwdRateSeries = new Series("Forward Rate")
            {
                ChartType = SeriesChartType.Line,
                BorderWidth = 2,
                Color = Color.OrangeRed,
                BorderDashStyle = ChartDashStyle.Dash,
                MarkerStyle = MarkerStyle.Diamond,
                MarkerSize = 5,
                MarkerColor = Color.OrangeRed
            };
            curveChart.Series.Add(fwdRateSeries);

            var legend = new Legend
            {
                Docking = Docking.Top,
                Alignment = StringAlignment.Center
            };
            curveChart.Legends.Add(legend);

            splitContainer.Panel1.Controls.Add(curveChart);

            // Data grid for curve points
            dataGrid = new DataGridView
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

            dataGrid.Columns.Add("Tenor", "Tenor (Y)");
            dataGrid.Columns.Add("ZeroRate", "Zero Rate (%)");
            dataGrid.Columns.Add("DF", "Discount Factor");
            dataGrid.Columns.Add("FwdRate", "Fwd Rate (%)");

            dataGrid.Columns["Tenor"].Width = 80;
            dataGrid.Columns["ZeroRate"].Width = 100;
            dataGrid.Columns["DF"].Width = 120;
            dataGrid.Columns["FwdRate"].Width = 100;

            splitContainer.Panel2.Controls.Add(dataGrid);

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
            this.Controls.Add(grpQuery);
            this.Controls.Add(topPanel);
            this.Controls.Add(buttonPanel);

            // Handle resize
            this.Resize += (s, e) =>
            {
                btnClose.Location = new Point(buttonPanel.Width - btnClose.Width - 15, 8);
            };
        }

        private void RefreshCurveList()
        {
            cboCurves.Items.Clear();
            var curves = new List<CurveInfo>();

            // Enumerate curves from registry
            NativeMethods.ObjectEnumCallback callback = (handle, objType, namePtr) =>
            {
                if (objType == 1) // Curve type
                {
                    string name = namePtr != IntPtr.Zero
                        ? System.Runtime.InteropServices.Marshal.PtrToStringAnsi(namePtr)
                        : "";
                    curves.Add(new CurveInfo { Handle = handle, Name = name });
                }
            };

            NativeMethods.convex_enumerate_objects(callback, 1); // Filter for curves

            foreach (var curve in curves)
            {
                string displayName = string.IsNullOrEmpty(curve.Name)
                    ? HandleHelper.Format(curve.Handle)
                    : $"{curve.Name} ({HandleHelper.Format(curve.Handle)})";
                cboCurves.Items.Add(new ComboBoxItem { Text = displayName, Value = curve.Handle });
            }

            if (cboCurves.Items.Count > 0)
                cboCurves.SelectedIndex = 0;
            else
                ClearDisplay();

            GC.KeepAlive(callback);
        }

        private void CboCurves_SelectedIndexChanged(object sender, EventArgs e)
        {
            if (cboCurves.SelectedItem is ComboBoxItem item)
            {
                LoadCurveData(item.Value);
            }
        }

        private void LoadCurveData(ulong handle)
        {
            dataGrid.Rows.Clear();
            curveChart.Series["Zero Rate"].Points.Clear();
            curveChart.Series["Forward Rate"].Points.Clear();

            int count = NativeMethods.convex_curve_tenor_count(handle);
            if (count <= 0)
            {
                lblCurveInfo.Text = "No data";
                return;
            }

            int refDateInt = NativeMethods.convex_curve_ref_date(handle);
            if (refDateInt > 0)
            {
                int year = refDateInt / 10000;
                int month = (refDateInt / 100) % 100;
                int day = refDateInt % 100;
                lblCurveInfo.Text = $"Ref: {year}-{month:D2}-{day:D2}, Points: {count}";
            }
            else
            {
                lblCurveInfo.Text = $"Points: {count}";
            }

            double prevTenor = 0;
            for (int i = 0; i < count; i++)
            {
                double tenor = NativeMethods.convex_curve_get_tenor(handle, i);
                double rate = NativeMethods.convex_curve_get_rate(handle, i);
                double df = NativeMethods.convex_curve_df(handle, tenor);

                // Calculate forward rate from previous tenor
                double fwdRate = double.NaN;
                if (i > 0 && tenor > prevTenor)
                {
                    fwdRate = NativeMethods.convex_curve_forward_rate(handle, prevTenor, tenor);
                }

                // Add to grid
                dataGrid.Rows.Add(
                    tenor.ToString("F4"),
                    (rate * 100).ToString("F4"),
                    df.ToString("F6"),
                    double.IsNaN(fwdRate) ? "-" : (fwdRate * 100).ToString("F4")
                );

                // Add to chart
                curveChart.Series["Zero Rate"].Points.AddXY(tenor, rate * 100);
                if (!double.IsNaN(fwdRate))
                {
                    curveChart.Series["Forward Rate"].Points.AddXY(tenor, fwdRate * 100);
                }

                prevTenor = tenor;
            }
        }

        private void BtnQuery_Click(object sender, EventArgs e)
        {
            if (cboCurves.SelectedItem is ComboBoxItem item)
            {
                double tenor = (double)numTenor.Value;
                double zeroRate = NativeMethods.convex_curve_zero_rate(item.Value, tenor);
                double df = NativeMethods.convex_curve_df(item.Value, tenor);

                lblZeroRateValue.Text = double.IsNaN(zeroRate) ? "N/A" : $"{zeroRate * 100:F4}%";
                lblDFValue.Text = double.IsNaN(df) ? "N/A" : df.ToString("F6");
            }
        }

        private void ClearDisplay()
        {
            dataGrid.Rows.Clear();
            curveChart.Series["Zero Rate"].Points.Clear();
            curveChart.Series["Forward Rate"].Points.Clear();
            lblCurveInfo.Text = "No curves";
            lblZeroRateValue.Text = "-";
            lblDFValue.Text = "-";
        }

        private class CurveInfo
        {
            public ulong Handle { get; set; }
            public string Name { get; set; }
        }

        private class ComboBoxItem
        {
            public string Text { get; set; }
            public ulong Value { get; set; }
            public override string ToString() => Text;
        }
    }
}
