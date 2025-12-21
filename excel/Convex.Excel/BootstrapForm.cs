using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;

namespace Convex.Excel
{
    /// <summary>
    /// Form for bootstrapping yield curves from market instruments.
    /// Supports deposits, FRAs, swaps, and OIS instruments.
    /// </summary>
    public class BootstrapForm : Form
    {
        private TextBox txtName;
        private DateTimePicker dtpRefDate;
        private ComboBox cboCurveType;
        private ComboBox cboMethod;
        private ComboBox cboInterpolation;
        private ComboBox cboDayCount;
        private DataGridView dataGrid;
        private Button btnBootstrap;
        private Button btnCancel;
        private Button btnLoadSample;
        private Label lblResult;
        private Label lblStatus;

        public BootstrapForm()
        {
            InitializeComponent();
        }

        private void InitializeComponent()
        {
            this.Text = "Bootstrap Curve";
            this.Size = new Size(600, 550);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;
            this.MinimizeBox = false;

            int y = 15;

            // Name
            var lblName = new Label { Text = "Curve Name:", Location = new Point(20, y), AutoSize = true };
            txtName = new TextBox { Location = new Point(120, y - 3), Width = 180 };
            y += 30;

            // Reference date
            var lblRefDate = new Label { Text = "Reference Date:", Location = new Point(20, y), AutoSize = true };
            dtpRefDate = new DateTimePicker
            {
                Location = new Point(120, y - 3),
                Width = 120,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today
            };
            y += 30;

            // Curve type
            var lblCurveType = new Label { Text = "Curve Type:", Location = new Point(20, y), AutoSize = true };
            cboCurveType = new ComboBox
            {
                Location = new Point(120, y - 3),
                Width = 150,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboCurveType.Items.AddRange(new[] {
                "Deposit + Swap",
                "OIS Only",
                "Mixed Instruments"
            });
            cboCurveType.SelectedIndex = 0;
            cboCurveType.SelectedIndexChanged += CboCurveType_SelectedIndexChanged;

            // Bootstrap method
            var lblMethod = new Label { Text = "Method:", Location = new Point(300, y), AutoSize = true };
            cboMethod = new ComboBox
            {
                Location = new Point(360, y - 3),
                Width = 130,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboMethod.Items.AddRange(new[] { "Global Fit (L-M)", "Piecewise (Brent)" });
            cboMethod.SelectedIndex = 0;
            y += 30;

            // Interpolation
            var lblInterp = new Label { Text = "Interpolation:", Location = new Point(20, y), AutoSize = true };
            cboInterpolation = new ComboBox
            {
                Location = new Point(120, y - 3),
                Width = 130,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboInterpolation.Items.AddRange(new[] { "Linear", "Log-Linear", "Cubic", "Monotone Convex" });
            cboInterpolation.SelectedIndex = 0;

            // Day count
            var lblDayCount = new Label { Text = "Day Count:", Location = new Point(280, y), AutoSize = true };
            cboDayCount = new ComboBox
            {
                Location = new Point(360, y - 3),
                Width = 120,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboDayCount.Items.AddRange(new[] { "ACT/360", "ACT/365", "ACT/ACT ISDA", "ACT/ACT ICMA", "30/360 US", "30E/360" });
            cboDayCount.SelectedIndex = 0;
            y += 35;

            // Data grid label with sample button
            var lblData = new Label { Text = "Market Instruments:", Location = new Point(20, y), AutoSize = true };
            btnLoadSample = new Button
            {
                Text = "Load Sample Data",
                Location = new Point(440, y - 5),
                Width = 120,
                Height = 25
            };
            btnLoadSample.Click += BtnLoadSample_Click;
            y += 25;

            // Data grid for instruments
            dataGrid = new DataGridView
            {
                Location = new Point(20, y),
                Size = new Size(540, 280),
                AllowUserToDeleteRows = true,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                RowHeadersVisible = true,
                BackgroundColor = SystemColors.Window
            };

            SetupGridForDepositSwap();

            y += 290;

            // Status label
            lblStatus = new Label
            {
                Location = new Point(20, y),
                Size = new Size(540, 20),
                ForeColor = Color.Gray,
                Text = "Enter instrument data and click Bootstrap to build the curve."
            };
            y += 25;

            // Result label
            lblResult = new Label
            {
                Location = new Point(20, y),
                Size = new Size(540, 20),
                ForeColor = Color.DarkBlue
            };
            y += 25;

            // Buttons
            btnBootstrap = new Button
            {
                Text = "Bootstrap",
                Location = new Point(360, y),
                Width = 90,
                Height = 28
            };
            btnBootstrap.Click += BtnBootstrap_Click;

            btnCancel = new Button
            {
                Text = "Cancel",
                Location = new Point(460, y),
                Width = 90,
                Height = 28
            };
            btnCancel.Click += (s, e) => this.Close();

            this.Controls.AddRange(new Control[] {
                lblName, txtName, lblRefDate, dtpRefDate,
                lblCurveType, cboCurveType, lblMethod, cboMethod,
                lblInterp, cboInterpolation, lblDayCount, cboDayCount,
                lblData, btnLoadSample, dataGrid, lblStatus, lblResult,
                btnBootstrap, btnCancel
            });
        }

        private void SetupGridForDepositSwap()
        {
            dataGrid.Columns.Clear();
            dataGrid.Rows.Clear();

            var typeCol = new DataGridViewComboBoxColumn
            {
                Name = "Type",
                HeaderText = "Type",
                Items = { "Deposit", "Swap" },
                Width = 80
            };
            dataGrid.Columns.Add(typeCol);
            dataGrid.Columns.Add("Tenor", "Tenor (Years)");
            dataGrid.Columns.Add("Rate", "Rate (%)");
        }

        private void SetupGridForOIS()
        {
            dataGrid.Columns.Clear();
            dataGrid.Rows.Clear();

            dataGrid.Columns.Add("Tenor", "Tenor (Years)");
            dataGrid.Columns.Add("Rate", "OIS Rate (%)");
        }

        private void SetupGridForMixed()
        {
            dataGrid.Columns.Clear();
            dataGrid.Rows.Clear();

            var typeCol = new DataGridViewComboBoxColumn
            {
                Name = "Type",
                HeaderText = "Type",
                Items = { "Deposit", "FRA", "Swap", "OIS" },
                Width = 80
            };
            dataGrid.Columns.Add(typeCol);
            dataGrid.Columns.Add("Tenor", "Tenor (Years)");
            dataGrid.Columns.Add("Rate", "Rate (%)");
        }

        private void CboCurveType_SelectedIndexChanged(object sender, EventArgs e)
        {
            switch (cboCurveType.SelectedIndex)
            {
                case 0: // Deposit + Swap
                    SetupGridForDepositSwap();
                    break;
                case 1: // OIS Only
                    SetupGridForOIS();
                    break;
                case 2: // Mixed
                    SetupGridForMixed();
                    break;
            }
        }

        private void BtnLoadSample_Click(object sender, EventArgs e)
        {
            switch (cboCurveType.SelectedIndex)
            {
                case 0: // Deposit + Swap
                    SetupGridForDepositSwap();
                    // Sample USD SOFR-like curve
                    dataGrid.Rows.Add("Deposit", "0.083", "4.50");  // 1M
                    dataGrid.Rows.Add("Deposit", "0.25", "4.55");   // 3M
                    dataGrid.Rows.Add("Deposit", "0.5", "4.60");    // 6M
                    dataGrid.Rows.Add("Deposit", "1.0", "4.65");    // 1Y
                    dataGrid.Rows.Add("Swap", "2.0", "4.20");       // 2Y
                    dataGrid.Rows.Add("Swap", "3.0", "4.00");       // 3Y
                    dataGrid.Rows.Add("Swap", "5.0", "3.85");       // 5Y
                    dataGrid.Rows.Add("Swap", "7.0", "3.80");       // 7Y
                    dataGrid.Rows.Add("Swap", "10.0", "3.75");      // 10Y
                    dataGrid.Rows.Add("Swap", "20.0", "3.85");      // 20Y
                    dataGrid.Rows.Add("Swap", "30.0", "3.90");      // 30Y
                    break;

                case 1: // OIS Only
                    SetupGridForOIS();
                    // Sample SOFR OIS curve
                    dataGrid.Rows.Add("0.083", "4.30");  // 1M
                    dataGrid.Rows.Add("0.25", "4.35");   // 3M
                    dataGrid.Rows.Add("0.5", "4.40");    // 6M
                    dataGrid.Rows.Add("1.0", "4.45");    // 1Y
                    dataGrid.Rows.Add("2.0", "4.15");    // 2Y
                    dataGrid.Rows.Add("3.0", "3.95");    // 3Y
                    dataGrid.Rows.Add("5.0", "3.80");    // 5Y
                    dataGrid.Rows.Add("10.0", "3.70");   // 10Y
                    break;

                case 2: // Mixed
                    SetupGridForMixed();
                    dataGrid.Rows.Add("Deposit", "0.25", "4.55");
                    dataGrid.Rows.Add("Deposit", "0.5", "4.60");
                    dataGrid.Rows.Add("FRA", "0.75", "4.50");
                    dataGrid.Rows.Add("Swap", "2.0", "4.20");
                    dataGrid.Rows.Add("Swap", "5.0", "3.85");
                    dataGrid.Rows.Add("OIS", "10.0", "3.70");
                    break;
            }

            lblStatus.Text = "Sample data loaded. Click Bootstrap to build the curve.";
            lblStatus.ForeColor = Color.Green;
        }

        private void BtnBootstrap_Click(object sender, EventArgs e)
        {
            try
            {
                lblStatus.Text = "Bootstrapping...";
                lblStatus.ForeColor = Color.Blue;
                lblResult.Text = "";
                Application.DoEvents();

                string name = string.IsNullOrWhiteSpace(txtName.Text) ? null : txtName.Text.Trim();
                DateTime refDate = dtpRefDate.Value;
                int interpolation = cboInterpolation.SelectedIndex;
                int dayCount = cboDayCount.SelectedIndex;

                ulong handle = NativeMethods.INVALID_HANDLE;

                switch (cboCurveType.SelectedIndex)
                {
                    case 0: // Deposit + Swap
                        handle = BootstrapDepositSwap(name, refDate, interpolation, dayCount);
                        break;
                    case 1: // OIS Only
                        handle = BootstrapOIS(name, refDate, interpolation, dayCount);
                        break;
                    case 2: // Mixed
                        handle = BootstrapMixed(name, refDate, interpolation, dayCount);
                        break;
                }

                if (handle != NativeMethods.INVALID_HANDLE)
                {
                    string handleStr = HandleHelper.Format(handle);
                    lblResult.Text = $"Curve created: {handleStr}";
                    lblResult.ForeColor = Color.DarkGreen;
                    lblStatus.Text = "Bootstrap completed successfully.";
                    lblStatus.ForeColor = Color.Green;

                    // Show quick curve info
                    double rate5y = NativeMethods.convex_curve_zero_rate(handle, 5.0);
                    if (!double.IsNaN(rate5y))
                    {
                        lblStatus.Text += $"  5Y zero rate: {rate5y * 100:F3}%";
                    }
                }
                else
                {
                    string errorMsg = ConvexWrapper.GetLastError();
                    lblResult.Text = string.IsNullOrEmpty(errorMsg)
                        ? "Bootstrap failed"
                        : $"Error: {errorMsg}";
                    lblResult.ForeColor = Color.Red;
                    lblStatus.Text = "Bootstrap failed.";
                    lblStatus.ForeColor = Color.Red;
                }
            }
            catch (Exception ex)
            {
                lblResult.Text = $"Error: {ex.Message}";
                lblResult.ForeColor = Color.Red;
                lblStatus.Text = "An exception occurred.";
                lblStatus.ForeColor = Color.Red;
            }
        }

        private ulong BootstrapDepositSwap(string name, DateTime refDate, int interpolation, int dayCount)
        {
            var depositTenors = new List<double>();
            var depositRates = new List<double>();
            var swapTenors = new List<double>();
            var swapRates = new List<double>();

            foreach (DataGridViewRow row in dataGrid.Rows)
            {
                if (row.IsNewRow) continue;

                var typeCell = row.Cells["Type"].Value;
                var tenorCell = row.Cells["Tenor"].Value;
                var rateCell = row.Cells["Rate"].Value;

                if (typeCell == null || tenorCell == null || rateCell == null)
                    continue;

                string type = typeCell.ToString();
                if (!double.TryParse(tenorCell.ToString(), out double tenor))
                    continue;
                if (!double.TryParse(rateCell.ToString(), out double rate))
                    continue;

                rate = rate / 100.0; // Convert from % to decimal

                if (type == "Deposit")
                {
                    depositTenors.Add(tenor);
                    depositRates.Add(rate);
                }
                else if (type == "Swap")
                {
                    swapTenors.Add(tenor);
                    swapRates.Add(rate);
                }
            }

            if (depositTenors.Count + swapTenors.Count < 2)
            {
                lblStatus.Text = "Need at least 2 instruments.";
                lblStatus.ForeColor = Color.Red;
                return NativeMethods.INVALID_HANDLE;
            }

            // Check if piecewise method is selected
            bool usePiecewise = cboMethod.SelectedIndex == 1;

            if (usePiecewise)
            {
                return NativeMethods.convex_bootstrap_piecewise(
                    name,
                    refDate.Year, refDate.Month, refDate.Day,
                    depositTenors.ToArray(),
                    depositRates.ToArray(),
                    depositTenors.Count,
                    swapTenors.ToArray(),
                    swapRates.ToArray(),
                    swapTenors.Count,
                    interpolation,
                    dayCount);
            }
            else
            {
                return NativeMethods.convex_bootstrap_from_instruments(
                    name,
                    refDate.Year, refDate.Month, refDate.Day,
                    depositTenors.ToArray(),
                    depositRates.ToArray(),
                    depositTenors.Count,
                    swapTenors.ToArray(),
                    swapRates.ToArray(),
                    swapTenors.Count,
                    interpolation,
                    dayCount);
            }
        }

        private ulong BootstrapOIS(string name, DateTime refDate, int interpolation, int dayCount)
        {
            var tenors = new List<double>();
            var rates = new List<double>();

            foreach (DataGridViewRow row in dataGrid.Rows)
            {
                if (row.IsNewRow) continue;

                var tenorCell = row.Cells["Tenor"].Value;
                var rateCell = row.Cells["Rate"].Value;

                if (tenorCell == null || rateCell == null)
                    continue;

                if (!double.TryParse(tenorCell.ToString(), out double tenor))
                    continue;
                if (!double.TryParse(rateCell.ToString(), out double rate))
                    continue;

                tenors.Add(tenor);
                rates.Add(rate / 100.0);
            }

            if (tenors.Count < 2)
            {
                lblStatus.Text = "Need at least 2 OIS instruments.";
                lblStatus.ForeColor = Color.Red;
                return NativeMethods.INVALID_HANDLE;
            }

            return NativeMethods.convex_bootstrap_ois(
                name,
                refDate.Year, refDate.Month, refDate.Day,
                tenors.ToArray(),
                rates.ToArray(),
                tenors.Count,
                interpolation,
                dayCount);
        }

        private ulong BootstrapMixed(string name, DateTime refDate, int interpolation, int dayCount)
        {
            var types = new List<int>();
            var tenors = new List<double>();
            var rates = new List<double>();

            foreach (DataGridViewRow row in dataGrid.Rows)
            {
                if (row.IsNewRow) continue;

                var typeCell = row.Cells["Type"].Value;
                var tenorCell = row.Cells["Tenor"].Value;
                var rateCell = row.Cells["Rate"].Value;

                if (typeCell == null || tenorCell == null || rateCell == null)
                    continue;

                string typeStr = typeCell.ToString();
                if (!double.TryParse(tenorCell.ToString(), out double tenor))
                    continue;
                if (!double.TryParse(rateCell.ToString(), out double rate))
                    continue;

                int typeInt = typeStr switch
                {
                    "Deposit" => 0,
                    "FRA" => 1,
                    "Swap" => 2,
                    "OIS" => 3,
                    _ => -1
                };

                if (typeInt < 0) continue;

                types.Add(typeInt);
                tenors.Add(tenor);
                rates.Add(rate / 100.0);
            }

            if (tenors.Count < 2)
            {
                lblStatus.Text = "Need at least 2 instruments.";
                lblStatus.ForeColor = Color.Red;
                return NativeMethods.INVALID_HANDLE;
            }

            // Check if piecewise method is selected
            bool usePiecewise = cboMethod.SelectedIndex == 1;

            if (usePiecewise)
            {
                return NativeMethods.convex_bootstrap_piecewise_mixed(
                    name,
                    refDate.Year, refDate.Month, refDate.Day,
                    types.ToArray(),
                    tenors.ToArray(),
                    rates.ToArray(),
                    tenors.Count,
                    interpolation,
                    dayCount);
            }
            else
            {
                return NativeMethods.convex_bootstrap_mixed(
                    name,
                    refDate.Year, refDate.Month, refDate.Day,
                    types.ToArray(),
                    tenors.ToArray(),
                    rates.ToArray(),
                    tenors.Count,
                    interpolation,
                    dayCount);
            }
        }
    }
}
