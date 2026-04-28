using System;
using System.Drawing;
using System.Globalization;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;
using static Convex.Excel.Helpers.BondSpecs;

namespace Convex.Excel.Forms
{
    // Build any BondSpec interactively. One tab per shape; each tab knows
    // exactly which fields its variant carries. The spec JSON is identical
    // to what =CX.BOND / =CX.BOND.CALLABLE etc. would emit, so handles built
    // via the form and via UDFs are interchangeable.
    internal sealed class BondBuilderForm : Form
    {
        private readonly TabControl _tabs = new() { Dock = DockStyle.Fill };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        private readonly FixedRateTab _fixedTab = new();
        private readonly CallableTab _callableTab = new();
        private readonly FrnTab _frnTab = new();
        private readonly ZeroTab _zeroTab = new();

        public BondBuilderForm()
        {
            Text = "Convex — New Bond";
            Size = new Size(560, 540);
            MinimumSize = new Size(500, 460);
            StartPosition = FormStartPosition.CenterParent;

            _tabs.TabPages.Add(_fixedTab);
            _tabs.TabPages.Add(_callableTab);
            _tabs.TabPages.Add(_frnTab);
            _tabs.TabPages.Add(_zeroTab);

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom,
                Height = 44,
                Padding = new Padding(10, 6, 10, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Build", (_, _) => Build()),
                NewButton("Build + paste handle", (_, _) => BuildAndPaste()),
                _status,
                NewButton("Close", (_, _) => Close()),
            });

            Controls.Add(_tabs);
            Controls.Add(bottom);
        }

        private JObject CurrentSpec()
        {
            var page = _tabs.SelectedTab as BondTab
                ?? throw new ConvexException("no bond tab selected");
            return page.BuildSpec();
        }

        private void Build()
        {
            try
            {
                var handle = Cx.BuildBond(CurrentSpec());
                _status.Text = $"OK — {CxParse.FormatHandle(handle)}";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        private void BuildAndPaste()
        {
            try
            {
                var handle = Cx.BuildBond(CurrentSpec());
                var fmt = CxParse.FormatHandle(handle);
                var addr = SheetHelpers.WriteFormulaAtSelection("=\"" + fmt + "\"");
                _status.Text = $"Pasted {fmt} at {addr}";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        private static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }

        // ============================================================
        // Per-shape tabs
        // ============================================================

        private abstract class BondTab : TabPage
        {
            public abstract JObject BuildSpec();

            protected static TableLayoutPanel NewGrid(int rows)
            {
                var t = new TableLayoutPanel
                {
                    Dock = DockStyle.Fill,
                    ColumnCount = 2,
                    RowCount = rows,
                    Padding = new Padding(10),
                };
                t.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
                t.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
                for (int i = 0; i < rows; i++)
                    t.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
                return t;
            }

            protected static double Parse(TextBox tb, string field)
            {
                if (!double.TryParse(tb.Text.Trim(), NumberStyles.Any, CultureInfo.InvariantCulture, out var d))
                    throw new ConvexException(field + ": " + tb.Text + " is not a number");
                return d;
            }
        }

        private sealed class FixedRateTab : BondTab
        {
            private readonly TextBox _id = new();
            private readonly TextBox _coupon = new() { Text = "0.05" };
            private readonly DateTimePicker _maturity = NewDate(10);
            private readonly DateTimePicker _issue = NewDate(0);
            private readonly ComboBox _frequency = NewFrequency();
            private readonly ComboBox _dayCount = NewDayCount("Thirty360US");
            private readonly TextBox _faceValue = new() { Text = "100" };

            public FixedRateTab() : base()
            {
                Text = "Fixed Rate";
                var g = NewGrid(7);
                AddRow(g, 0, "ID (CUSIP/ISIN/name):", _id);
                AddRow(g, 1, "Coupon rate (decimal):", _coupon);
                AddRow(g, 2, "Maturity:", _maturity);
                AddRow(g, 3, "Issue:", _issue);
                AddRow(g, 4, "Frequency:", _frequency);
                AddRow(g, 5, "Day count:", _dayCount);
                AddRow(g, 6, "Face value:", _faceValue);
                Controls.Add(g);
            }

            public override JObject BuildSpec() => FixedRate(
                _id.Text,
                Parse(_coupon, "coupon_rate"),
                (string)_frequency.SelectedItem!,
                _maturity.Value, _issue.Value,
                (string)_dayCount.SelectedItem!,
                "USD",
                Parse(_faceValue, "face_value"));
        }

        private sealed class CallableTab : BondTab
        {
            private readonly TextBox _id = new();
            private readonly TextBox _coupon = new() { Text = "0.05" };
            private readonly DateTimePicker _maturity = NewDate(10);
            private readonly DateTimePicker _issue = NewDate(0);
            private readonly ComboBox _frequency = NewFrequency();
            private readonly ComboBox _dayCount = NewDayCount("Thirty360US");
            private readonly ComboBox _style = new()
            {
                DropDownStyle = ComboBoxStyle.DropDownList,
                Anchor = AnchorStyles.Left | AnchorStyles.Right,
            };
            private readonly DataGridView _schedule = new()
            {
                Anchor = AnchorStyles.Left | AnchorStyles.Right,
                Height = 130,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                AllowUserToResizeRows = false,
                RowHeadersVisible = false,
            };

            public CallableTab() : base()
            {
                Text = "Callable";
                _style.Items.AddRange(new object[] { "american", "european", "bermudan" });
                _style.SelectedIndex = 0;
                _schedule.Columns.Add("date", "Call date (yyyy-mm-dd)");
                _schedule.Columns.Add("price", "Call price (% par)");

                var g = NewGrid(8);
                AddRow(g, 0, "ID:", _id);
                AddRow(g, 1, "Coupon:", _coupon);
                AddRow(g, 2, "Maturity:", _maturity);
                AddRow(g, 3, "Issue:", _issue);
                AddRow(g, 4, "Frequency:", _frequency);
                AddRow(g, 5, "Day count:", _dayCount);
                AddRow(g, 6, "Style:", _style);
                AddRow(g, 7, "Schedule:", _schedule);
                Controls.Add(g);
            }

            public override JObject BuildSpec()
            {
                var schedule = new JArray();
                foreach (DataGridViewRow row in _schedule.Rows)
                {
                    if (row.IsNewRow) continue;
                    var date = (row.Cells[0].Value ?? "").ToString()!.Trim();
                    var priceText = (row.Cells[1].Value ?? "").ToString()!.Trim();
                    if (date.Length == 0 || priceText.Length == 0) continue;
                    if (!double.TryParse(priceText, NumberStyles.Any, CultureInfo.InvariantCulture, out var p))
                        throw new ConvexException("call_schedule: invalid price " + priceText);
                    schedule.Add(new JObject { ["date"] = date, ["price"] = p });
                }
                if (schedule.Count == 0)
                    throw new ConvexException("call_schedule must have at least one entry");
                return Callable(
                    _id.Text,
                    Parse(_coupon, "coupon_rate"),
                    (string)_frequency.SelectedItem!,
                    _maturity.Value, _issue.Value,
                    schedule,
                    (string)_style.SelectedItem!,
                    (string)_dayCount.SelectedItem!);
            }
        }

        private sealed class FrnTab : BondTab
        {
            private readonly TextBox _id = new();
            private readonly TextBox _spread = new() { Text = "75" };
            private readonly DateTimePicker _maturity = NewDate(5);
            private readonly DateTimePicker _issue = NewDate(0);
            private readonly ComboBox _index = new() { DropDownStyle = ComboBoxStyle.DropDownList, Anchor = AnchorStyles.Left | AnchorStyles.Right };
            private readonly ComboBox _frequency = NewFrequency("Quarterly");
            private readonly ComboBox _dayCount = NewDayCount("Act360");
            private readonly TextBox _cap = new();
            private readonly TextBox _floor = new();

            public FrnTab() : base()
            {
                Text = "FRN";
                _index.Items.AddRange(new object[] { "sofr", "sonia", "estr", "tonar", "saron", "corra", "euribor3m", "euribor6m", "tibor3m" });
                _index.SelectedIndex = 0;
                var g = NewGrid(9);
                AddRow(g, 0, "ID:", _id);
                AddRow(g, 1, "Spread (bps):", _spread);
                AddRow(g, 2, "Maturity:", _maturity);
                AddRow(g, 3, "Issue:", _issue);
                AddRow(g, 4, "Index:", _index);
                AddRow(g, 5, "Frequency:", _frequency);
                AddRow(g, 6, "Day count:", _dayCount);
                AddRow(g, 7, "Cap (decimal):", _cap);
                AddRow(g, 8, "Floor (decimal):", _floor);
                Controls.Add(g);
            }

            public override JObject BuildSpec() => Frn(
                _id.Text,
                Parse(_spread, "spread_bps"),
                _maturity.Value, _issue.Value,
                (string)_index.SelectedItem!,
                (string)_frequency.SelectedItem!,
                (string)_dayCount.SelectedItem!,
                _cap.Text.Trim().Length > 0 ? Parse(_cap, "cap") : (double?)null,
                _floor.Text.Trim().Length > 0 ? Parse(_floor, "floor") : (double?)null);
        }

        private sealed class ZeroTab : BondTab
        {
            private readonly TextBox _id = new();
            private readonly DateTimePicker _maturity = NewDate(10);
            private readonly DateTimePicker _issue = NewDate(0);
            private readonly ComboBox _compounding = new()
            {
                DropDownStyle = ComboBoxStyle.DropDownList,
                Anchor = AnchorStyles.Left | AnchorStyles.Right,
            };
            private readonly ComboBox _dayCount = NewDayCount("ActActIcma");

            public ZeroTab() : base()
            {
                Text = "Zero Coupon";
                _compounding.Items.AddRange(new object[] { "Annual", "SemiAnnual", "Quarterly", "Monthly", "Continuous", "Simple" });
                _compounding.SelectedIndex = 1;
                var g = NewGrid(5);
                AddRow(g, 0, "ID:", _id);
                AddRow(g, 1, "Maturity:", _maturity);
                AddRow(g, 2, "Issue:", _issue);
                AddRow(g, 3, "Compounding:", _compounding);
                AddRow(g, 4, "Day count:", _dayCount);
                Controls.Add(g);
            }

            public override JObject BuildSpec() => ZeroCoupon(
                _id.Text,
                _maturity.Value, _issue.Value,
                (string)_compounding.SelectedItem!,
                (string)_dayCount.SelectedItem!);
        }

        private static void AddRow(TableLayoutPanel grid, int row, string label, Control control)
        {
            grid.Controls.Add(new Label { Text = label, Anchor = AnchorStyles.Left, AutoSize = true, Padding = new Padding(0, 6, 0, 0) }, 0, row);
            control.Anchor = AnchorStyles.Left | AnchorStyles.Right;
            grid.Controls.Add(control, 1, row);
        }

        private static DateTimePicker NewDate(int yearsFromToday)
        {
            var dt = new DateTimePicker { Format = DateTimePickerFormat.Short };
            dt.Value = DateTime.Today.AddYears(yearsFromToday);
            return dt;
        }

        private static ComboBox NewFrequency(string defaultValue = "SemiAnnual")
        {
            var c = new ComboBox { DropDownStyle = ComboBoxStyle.DropDownList };
            c.Items.AddRange(new object[] { "Annual", "SemiAnnual", "Quarterly", "Monthly" });
            c.SelectedItem = defaultValue;
            return c;
        }

        private static ComboBox NewDayCount(string defaultValue)
        {
            var c = new ComboBox { DropDownStyle = ComboBoxStyle.DropDownList };
            c.Items.AddRange(new object[] { "Act360", "Act365Fixed", "ActActIsda", "ActActIcma", "Thirty360US", "Thirty360E" });
            c.SelectedItem = defaultValue;
            return c;
        }
    }
}
