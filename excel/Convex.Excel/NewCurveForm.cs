using System;
using System.Drawing;
using System.Windows.Forms;

namespace Convex.Excel
{
    /// <summary>
    /// Form to create a new yield curve.
    /// </summary>
    public class NewCurveForm : Form
    {
        private TextBox txtName;
        private DateTimePicker dtpRefDate;
        private DataGridView dataGrid;
        private ComboBox cboInterpolation;
        private ComboBox cboDayCount;
        private Button btnCreate;
        private Button btnCancel;
        private Label lblResult;

        public NewCurveForm()
        {
            InitializeComponent();
        }

        private void InitializeComponent()
        {
            this.Text = "Create New Curve";
            this.Size = new Size(500, 450);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;
            this.MinimizeBox = false;

            // Name
            var lblName = new Label { Text = "Name:", Location = new Point(20, 20), AutoSize = true };
            txtName = new TextBox { Location = new Point(120, 17), Width = 200 };

            // Reference date
            var lblRefDate = new Label { Text = "Reference Date:", Location = new Point(20, 50), AutoSize = true };
            dtpRefDate = new DateTimePicker
            {
                Location = new Point(120, 47),
                Width = 120,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today
            };

            // Interpolation
            var lblInterp = new Label { Text = "Interpolation:", Location = new Point(20, 80), AutoSize = true };
            cboInterpolation = new ComboBox
            {
                Location = new Point(120, 77),
                Width = 120,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboInterpolation.Items.AddRange(new[] { "Linear", "Log-Linear", "Cubic" });
            cboInterpolation.SelectedIndex = 0;

            // Day count
            var lblDayCount = new Label { Text = "Day Count:", Location = new Point(260, 80), AutoSize = true };
            cboDayCount = new ComboBox
            {
                Location = new Point(340, 77),
                Width = 120,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboDayCount.Items.AddRange(new[] { "ACT/360", "ACT/365", "ACT/ACT", "ACT/ACT ICMA", "30/360", "30E/360" });
            cboDayCount.SelectedIndex = 1;

            // Data grid for tenor/rate pairs
            var lblData = new Label { Text = "Curve Points:", Location = new Point(20, 115), AutoSize = true };

            dataGrid = new DataGridView
            {
                Location = new Point(20, 135),
                Size = new Size(440, 200),
                AllowUserToDeleteRows = true,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                RowHeadersVisible = true,
                BackgroundColor = SystemColors.Window
            };

            dataGrid.Columns.Add("Tenor", "Tenor (Years)");
            dataGrid.Columns.Add("Rate", "Zero Rate (%)");

            // Add some default rows
            dataGrid.Rows.Add("1", "4.00");
            dataGrid.Rows.Add("2", "4.25");
            dataGrid.Rows.Add("5", "4.50");
            dataGrid.Rows.Add("10", "4.75");
            dataGrid.Rows.Add("30", "5.00");

            // Result label
            lblResult = new Label
            {
                Location = new Point(20, 345),
                AutoSize = true,
                ForeColor = Color.DarkBlue
            };

            // Buttons
            btnCreate = new Button
            {
                Text = "Create",
                Location = new Point(280, 370),
                Width = 80
            };
            btnCreate.Click += BtnCreate_Click;

            btnCancel = new Button
            {
                Text = "Cancel",
                Location = new Point(380, 370),
                Width = 80
            };
            btnCancel.Click += (s, e) => this.Close();

            this.Controls.AddRange(new Control[] {
                lblName, txtName, lblRefDate, dtpRefDate,
                lblInterp, cboInterpolation, lblDayCount, cboDayCount,
                lblData, dataGrid, lblResult, btnCreate, btnCancel
            });
        }

        private void BtnCreate_Click(object sender, EventArgs e)
        {
            try
            {
                // Collect data from grid
                var tenors = new System.Collections.Generic.List<double>();
                var rates = new System.Collections.Generic.List<double>();

                foreach (DataGridViewRow row in dataGrid.Rows)
                {
                    if (row.IsNewRow) continue;

                    var tenorCell = row.Cells["Tenor"].Value;
                    var rateCell = row.Cells["Rate"].Value;

                    if (tenorCell != null && rateCell != null)
                    {
                        if (double.TryParse(tenorCell.ToString(), out double tenor) &&
                            double.TryParse(rateCell.ToString(), out double rate))
                        {
                            tenors.Add(tenor);
                            rates.Add(rate / 100.0); // Convert from % to decimal
                        }
                    }
                }

                if (tenors.Count < 2)
                {
                    lblResult.Text = "Error: Need at least 2 data points";
                    lblResult.ForeColor = Color.Red;
                    return;
                }

                string name = string.IsNullOrWhiteSpace(txtName.Text) ? null : txtName.Text.Trim();
                DateTime refDate = dtpRefDate.Value;
                int interpolation = cboInterpolation.SelectedIndex;
                int dayCount = cboDayCount.SelectedIndex;

                // Create curve via FFI
                ulong handle = NativeMethods.convex_curve_from_zero_rates(
                    name,
                    refDate.Year, refDate.Month, refDate.Day,
                    tenors.ToArray(),
                    rates.ToArray(),
                    tenors.Count,
                    interpolation,
                    dayCount);

                if (handle != NativeMethods.INVALID_HANDLE)
                {
                    this.Close();
                    return;
                }
                else
                {
                    // Get last error message from FFI
                    string errorMsg = ConvexWrapper.GetLastError();
                    lblResult.Text = string.IsNullOrEmpty(errorMsg)
                        ? "Error creating curve"
                        : $"Error: {errorMsg}";
                    lblResult.ForeColor = Color.Red;
                }
            }
            catch (Exception ex)
            {
                lblResult.Text = $"Error: {ex.Message}";
                lblResult.ForeColor = Color.Red;
            }
        }
    }
}
