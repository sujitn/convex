using System;
using System.Drawing;
using System.Windows.Forms;

namespace Convex.Excel
{
    /// <summary>
    /// Form to create a new bond.
    /// </summary>
    public class NewBondForm : Form
    {
        private TextBox txtIdentifier;
        private NumericUpDown numCoupon;
        private NumericUpDown numFrequency;
        private DateTimePicker dtpMaturity;
        private DateTimePicker dtpIssue;
        private ComboBox cboBondType;
        private ComboBox cboDayCount;
        private GroupBox grpCallable;
        private CheckBox chkCallable;
        private DateTimePicker dtpCallDate;
        private NumericUpDown numCallPrice;
        private Button btnCreate;
        private Button btnCancel;
        private Label lblResult;

        public NewBondForm()
        {
            InitializeComponent();
        }

        private void InitializeComponent()
        {
            this.Text = "Create New Bond";
            this.Size = new Size(450, 420);
            this.StartPosition = FormStartPosition.CenterScreen;
            this.FormBorderStyle = FormBorderStyle.FixedDialog;
            this.MaximizeBox = false;
            this.MinimizeBox = false;

            // Bond type
            var lblType = new Label { Text = "Bond Type:", Location = new Point(20, 20), AutoSize = true };
            cboBondType = new ComboBox
            {
                Location = new Point(120, 17),
                Width = 150,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboBondType.Items.AddRange(new[] { "Generic Fixed", "US Corporate", "US Treasury" });
            cboBondType.SelectedIndex = 0;
            cboBondType.SelectedIndexChanged += CboBondType_SelectedIndexChanged;

            // Identifier
            var lblId = new Label { Text = "Identifier:", Location = new Point(20, 50), AutoSize = true };
            txtIdentifier = new TextBox { Location = new Point(120, 47), Width = 200 };

            // Coupon
            var lblCoupon = new Label { Text = "Coupon (%):", Location = new Point(20, 80), AutoSize = true };
            numCoupon = new NumericUpDown
            {
                Location = new Point(120, 77),
                Width = 80,
                DecimalPlaces = 3,
                Minimum = 0,
                Maximum = 20,
                Value = 5,
                Increment = 0.125m
            };

            // Frequency
            var lblFreq = new Label { Text = "Frequency:", Location = new Point(220, 80), AutoSize = true };
            numFrequency = new NumericUpDown
            {
                Location = new Point(290, 77),
                Width = 60,
                Minimum = 1,
                Maximum = 12,
                Value = 2
            };

            // Maturity
            var lblMaturity = new Label { Text = "Maturity:", Location = new Point(20, 110), AutoSize = true };
            dtpMaturity = new DateTimePicker
            {
                Location = new Point(120, 107),
                Width = 120,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today.AddYears(10)
            };

            // Issue
            var lblIssue = new Label { Text = "Issue Date:", Location = new Point(20, 140), AutoSize = true };
            dtpIssue = new DateTimePicker
            {
                Location = new Point(120, 137),
                Width = 120,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today.AddYears(-1)
            };

            // Day count
            var lblDayCount = new Label { Text = "Day Count:", Location = new Point(20, 170), AutoSize = true };
            cboDayCount = new ComboBox
            {
                Location = new Point(120, 167),
                Width = 120,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboDayCount.Items.AddRange(new[] { "ACT/360", "ACT/365", "ACT/ACT ISDA", "ACT/ACT ICMA", "30/360", "30E/360" });
            cboDayCount.SelectedIndex = 4; // 30/360 default

            // Callable options
            grpCallable = new GroupBox
            {
                Text = "Callable Options",
                Location = new Point(20, 200),
                Size = new Size(390, 80)
            };

            chkCallable = new CheckBox
            {
                Text = "Callable",
                Location = new Point(10, 20),
                AutoSize = true
            };
            chkCallable.CheckedChanged += ChkCallable_CheckedChanged;

            var lblCallDate = new Label { Text = "Call Date:", Location = new Point(100, 22), AutoSize = true };
            dtpCallDate = new DateTimePicker
            {
                Location = new Point(165, 19),
                Width = 100,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today.AddYears(5),
                Enabled = false
            };

            var lblCallPrice = new Label { Text = "Call Price:", Location = new Point(275, 22), AutoSize = true };
            numCallPrice = new NumericUpDown
            {
                Location = new Point(340, 19),
                Width = 40,
                DecimalPlaces = 0,
                Minimum = 100,
                Maximum = 110,
                Value = 100,
                Enabled = false
            };

            grpCallable.Controls.AddRange(new Control[] {
                chkCallable, lblCallDate, dtpCallDate, lblCallPrice, numCallPrice
            });

            // Result label
            lblResult = new Label
            {
                Location = new Point(20, 295),
                AutoSize = true,
                ForeColor = Color.DarkBlue
            };

            // Buttons
            btnCreate = new Button
            {
                Text = "Create",
                Location = new Point(240, 340),
                Width = 80
            };
            btnCreate.Click += BtnCreate_Click;

            btnCancel = new Button
            {
                Text = "Cancel",
                Location = new Point(340, 340),
                Width = 80
            };
            btnCancel.Click += (s, e) => this.Close();

            this.Controls.AddRange(new Control[] {
                lblType, cboBondType, lblId, txtIdentifier,
                lblCoupon, numCoupon, lblFreq, numFrequency,
                lblMaturity, dtpMaturity, lblIssue, dtpIssue,
                lblDayCount, cboDayCount, grpCallable,
                lblResult, btnCreate, btnCancel
            });
        }

        private void CboBondType_SelectedIndexChanged(object sender, EventArgs e)
        {
            // Adjust defaults based on bond type
            switch (cboBondType.SelectedIndex)
            {
                case 1: // US Corporate
                    cboDayCount.SelectedIndex = 4; // 30/360
                    numFrequency.Value = 2;
                    break;
                case 2: // US Treasury
                    cboDayCount.SelectedIndex = 2; // ACT/ACT
                    numFrequency.Value = 2;
                    break;
            }
        }

        private void ChkCallable_CheckedChanged(object sender, EventArgs e)
        {
            dtpCallDate.Enabled = chkCallable.Checked;
            numCallPrice.Enabled = chkCallable.Checked;
        }

        private void BtnCreate_Click(object sender, EventArgs e)
        {
            try
            {
                string identifier = string.IsNullOrWhiteSpace(txtIdentifier.Text) ? null : txtIdentifier.Text.Trim();
                double coupon = (double)numCoupon.Value;
                int frequency = (int)numFrequency.Value;
                DateTime maturity = dtpMaturity.Value;
                DateTime issue = dtpIssue.Value;
                int dayCount = cboDayCount.SelectedIndex;

                ulong handle;

                if (chkCallable.Checked)
                {
                    // Create callable bond
                    DateTime callDate = dtpCallDate.Value;
                    double callPrice = (double)numCallPrice.Value;

                    handle = NativeMethods.convex_bond_callable(
                        identifier,
                        coupon,
                        frequency,
                        maturity.Year, maturity.Month, maturity.Day,
                        issue.Year, issue.Month, issue.Day,
                        callDate.Year, callDate.Month, callDate.Day,
                        callPrice,
                        dayCount);
                }
                else
                {
                    // Create regular bond based on type
                    switch (cboBondType.SelectedIndex)
                    {
                        case 1: // US Corporate
                            handle = NativeMethods.convex_bond_us_corporate(
                                identifier,
                                coupon,
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day);
                            break;
                        case 2: // US Treasury
                            handle = NativeMethods.convex_bond_us_treasury(
                                identifier,
                                coupon,
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day);
                            break;
                        default: // Generic
                            handle = NativeMethods.convex_bond_fixed(
                                identifier,
                                coupon / 100.0, // Convert from % to decimal
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day,
                                frequency,
                                dayCount,
                                0,    // Currency: USD
                                100.0); // Face value
                            break;
                    }
                }

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
                        ? "Error creating bond"
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
