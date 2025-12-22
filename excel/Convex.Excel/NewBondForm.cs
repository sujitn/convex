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

        // FRN-specific controls
        private GroupBox grpFRN;
        private NumericUpDown numSpreadBps;
        private ComboBox cboRateIndex;
        private NumericUpDown numCap;
        private NumericUpDown numFloor;
        private CheckBox chkHasCap;
        private CheckBox chkHasFloor;

        // Zero coupon controls
        private ComboBox cboCompounding;

        // Labels that need to be hidden/shown
        private Label lblCoupon;
        private Label lblFreq;

        public NewBondForm()
        {
            InitializeComponent();
        }

        private void InitializeComponent()
        {
            this.Text = "Create New Bond";
            this.Size = new Size(450, 520);
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
            cboBondType.Items.AddRange(new[] {
                "Generic Fixed",
                "US Corporate",
                "US Treasury",
                "Zero Coupon",
                "US T-Bill",
                "Floating Rate Note",
                "US Treasury FRN"
            });
            cboBondType.SelectedIndex = 0;
            cboBondType.SelectedIndexChanged += CboBondType_SelectedIndexChanged;

            // Identifier
            var lblId = new Label { Text = "Identifier:", Location = new Point(20, 50), AutoSize = true };
            txtIdentifier = new TextBox { Location = new Point(120, 47), Width = 200 };

            // Coupon
            lblCoupon = new Label { Text = "Coupon (%):", Location = new Point(20, 80), AutoSize = true };
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
            lblFreq = new Label { Text = "Frequency:", Location = new Point(220, 80), AutoSize = true };
            numFrequency = new NumericUpDown
            {
                Location = new Point(290, 77),
                Width = 60,
                Minimum = 1,
                Maximum = 12,
                Value = 2
            };

            // Compounding (for zero coupon)
            var lblCompounding = new Label { Text = "Compounding:", Location = new Point(220, 80), AutoSize = true, Visible = false };
            cboCompounding = new ComboBox
            {
                Location = new Point(305, 77),
                Width = 100,
                DropDownStyle = ComboBoxStyle.DropDownList,
                Visible = false
            };
            cboCompounding.Items.AddRange(new[] { "Annual", "Semi-Annual", "Quarterly", "Monthly", "Continuous" });
            cboCompounding.SelectedIndex = 1; // Semi-Annual default

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

            // FRN options group
            grpFRN = new GroupBox
            {
                Text = "Floating Rate Options",
                Location = new Point(20, 200),
                Size = new Size(390, 85),
                Visible = false
            };

            var lblSpread = new Label { Text = "Spread (bps):", Location = new Point(10, 22), AutoSize = true };
            numSpreadBps = new NumericUpDown
            {
                Location = new Point(95, 19),
                Width = 60,
                DecimalPlaces = 1,
                Minimum = -500,
                Maximum = 1000,
                Value = 50,
                Increment = 5
            };

            var lblIndex = new Label { Text = "Index:", Location = new Point(170, 22), AutoSize = true };
            cboRateIndex = new ComboBox
            {
                Location = new Point(210, 19),
                Width = 110,
                DropDownStyle = ComboBoxStyle.DropDownList
            };
            cboRateIndex.Items.AddRange(new[] {
                "SOFR", "ESTR", "SONIA", "TONAR", "SARON", "CORRA", "AONIA", "HONIA",
                "EURIBOR 1M", "EURIBOR 3M", "EURIBOR 6M", "EURIBOR 12M", "TIBOR 3M"
            });
            cboRateIndex.SelectedIndex = 0; // SOFR default

            chkHasCap = new CheckBox { Text = "Cap (%):", Location = new Point(10, 52), AutoSize = true };
            numCap = new NumericUpDown
            {
                Location = new Point(95, 49),
                Width = 60,
                DecimalPlaces = 2,
                Minimum = 0,
                Maximum = 20,
                Value = 10,
                Enabled = false
            };
            chkHasCap.CheckedChanged += (s, e) => numCap.Enabled = chkHasCap.Checked;

            chkHasFloor = new CheckBox { Text = "Floor (%):", Location = new Point(170, 52), AutoSize = true };
            numFloor = new NumericUpDown
            {
                Location = new Point(250, 49),
                Width = 60,
                DecimalPlaces = 2,
                Minimum = 0,
                Maximum = 20,
                Value = 0,
                Enabled = false
            };
            chkHasFloor.CheckedChanged += (s, e) => numFloor.Enabled = chkHasFloor.Checked;

            grpFRN.Controls.AddRange(new Control[] {
                lblSpread, numSpreadBps, lblIndex, cboRateIndex,
                chkHasCap, numCap, chkHasFloor, numFloor
            });

            // Callable options
            grpCallable = new GroupBox
            {
                Text = "Callable Options",
                Location = new Point(20, 290),
                Size = new Size(390, 55)
            };

            chkCallable = new CheckBox
            {
                Text = "Callable",
                Location = new Point(10, 22),
                AutoSize = true
            };
            chkCallable.CheckedChanged += ChkCallable_CheckedChanged;

            var lblCallDate = new Label { Text = "Call Date:", Location = new Point(100, 24), AutoSize = true };
            dtpCallDate = new DateTimePicker
            {
                Location = new Point(165, 21),
                Width = 100,
                Format = DateTimePickerFormat.Short,
                Value = DateTime.Today.AddYears(5),
                Enabled = false
            };

            var lblCallPrice = new Label { Text = "Price:", Location = new Point(275, 24), AutoSize = true };
            numCallPrice = new NumericUpDown
            {
                Location = new Point(315, 21),
                Width = 55,
                DecimalPlaces = 1,
                Minimum = 100,
                Maximum = 120,
                Value = 100,
                Enabled = false
            };

            grpCallable.Controls.AddRange(new Control[] {
                chkCallable, lblCallDate, dtpCallDate, lblCallPrice, numCallPrice
            });

            // Result label
            lblResult = new Label
            {
                Location = new Point(20, 355),
                Size = new Size(390, 40),
                ForeColor = Color.DarkBlue
            };

            // Buttons
            btnCreate = new Button
            {
                Text = "Create",
                Location = new Point(240, 440),
                Width = 80
            };
            btnCreate.Click += BtnCreate_Click;

            btnCancel = new Button
            {
                Text = "Cancel",
                Location = new Point(340, 440),
                Width = 80
            };
            btnCancel.Click += (s, e) => this.Close();

            this.Controls.AddRange(new Control[] {
                lblType, cboBondType, lblId, txtIdentifier,
                lblCoupon, numCoupon, lblFreq, numFrequency,
                lblCompounding, cboCompounding,
                lblMaturity, dtpMaturity, lblIssue, dtpIssue,
                lblDayCount, cboDayCount, grpFRN, grpCallable,
                lblResult, btnCreate, btnCancel
            });
        }

        private void CboBondType_SelectedIndexChanged(object sender, EventArgs e)
        {
            // Get label references from Controls collection
            Label lblCompounding = null;
            foreach (Control c in this.Controls)
            {
                if (c is Label lbl && lbl.Text == "Compounding:")
                    lblCompounding = lbl;
            }

            // Adjust visibility and defaults based on bond type
            bool isZeroCoupon = cboBondType.SelectedIndex == 3; // Zero Coupon
            bool isTBill = cboBondType.SelectedIndex == 4; // US T-Bill
            bool isFRN = cboBondType.SelectedIndex == 5 || cboBondType.SelectedIndex == 6; // FRN types

            // Show/hide coupon and frequency for zero coupon / T-Bill
            bool showCoupon = !isZeroCoupon && !isTBill && !isFRN;
            lblCoupon.Visible = showCoupon;
            numCoupon.Visible = showCoupon;
            lblFreq.Visible = showCoupon && !isFRN;
            numFrequency.Visible = showCoupon && !isFRN;

            // Show/hide compounding for zero coupon
            if (lblCompounding != null) lblCompounding.Visible = isZeroCoupon;
            cboCompounding.Visible = isZeroCoupon;

            // Show/hide FRN options
            grpFRN.Visible = isFRN;

            // Show/hide callable options (not applicable to FRNs or T-Bills)
            grpCallable.Visible = !isFRN && !isTBill;

            // Adjust defaults based on bond type
            switch (cboBondType.SelectedIndex)
            {
                case 1: // US Corporate
                    cboDayCount.SelectedIndex = 4; // 30/360
                    numFrequency.Value = 2;
                    break;
                case 2: // US Treasury
                    cboDayCount.SelectedIndex = 2; // ACT/ACT ISDA
                    numFrequency.Value = 2;
                    break;
                case 3: // Zero Coupon
                    cboDayCount.SelectedIndex = 2; // ACT/ACT ISDA
                    break;
                case 4: // US T-Bill
                    cboDayCount.SelectedIndex = 0; // ACT/360
                    break;
                case 5: // Floating Rate Note
                    cboDayCount.SelectedIndex = 0; // ACT/360
                    numFrequency.Value = 4; // Quarterly
                    break;
                case 6: // US Treasury FRN
                    cboDayCount.SelectedIndex = 2; // ACT/ACT
                    numFrequency.Value = 4; // Quarterly
                    cboRateIndex.SelectedIndex = 0; // SOFR
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

                if (chkCallable.Checked && grpCallable.Visible)
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
                    // Create bond based on type
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
                        case 3: // Zero Coupon
                            int compounding = cboCompounding.SelectedIndex; // 0=Annual, 1=Semi, 2=Quarterly, 3=Monthly, 4=Continuous
                            handle = NativeMethods.convex_bond_zero_coupon(
                                identifier,
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day,
                                compounding,
                                dayCount,
                                0,    // Currency: USD
                                100.0); // Face value
                            break;
                        case 4: // US T-Bill
                            handle = NativeMethods.convex_bond_us_tbill(
                                identifier,
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day,
                                100.0); // Face value
                            break;
                        case 5: // Floating Rate Note
                            double spreadBps = (double)numSpreadBps.Value;
                            int rateIndex = cboRateIndex.SelectedIndex;
                            double capRate = chkHasCap.Checked ? (double)numCap.Value / 100.0 : 0.0;
                            double floorRate = chkHasFloor.Checked ? (double)numFloor.Value / 100.0 : 0.0;
                            handle = NativeMethods.convex_bond_frn(
                                identifier,
                                spreadBps,
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day,
                                frequency,
                                rateIndex,
                                dayCount,
                                0,    // Currency: USD
                                100.0, // Face value
                                capRate,
                                floorRate);
                            break;
                        case 6: // US Treasury FRN
                            handle = NativeMethods.convex_bond_us_treasury_frn(
                                identifier,
                                (double)numSpreadBps.Value,
                                maturity.Year, maturity.Month, maturity.Day,
                                issue.Year, issue.Month, issue.Day);
                            break;
                        default: // Generic Fixed
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
