using System;
using System.Drawing;
using System.Globalization;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;
using static Convex.Excel.Helpers.CurveSpecs;

namespace Convex.Excel.Forms
{
    // Build any CurveSpec interactively.
    //  • Discrete tab: explicit (tenor, value) pairs.
    //  • Bootstrap tab: deposits/FRAs/swaps/OIS — calls global_fit by default.
    internal sealed class CurveBuilderForm : Form
    {
        private readonly TabControl _tabs = new() { Dock = DockStyle.Fill };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };
        private readonly DiscreteTab _discrete = new();
        private readonly BootstrapTab _bootstrap = new();

        public CurveBuilderForm(bool startOnBootstrap = false)
        {
            Text = "Convex — New Curve";
            Size = new Size(620, 540);
            MinimumSize = new Size(540, 460);
            StartPosition = FormStartPosition.CenterParent;

            _tabs.TabPages.Add(_discrete);
            _tabs.TabPages.Add(_bootstrap);
            if (startOnBootstrap) _tabs.SelectedTab = _bootstrap;

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom,
                Height = 44,
                Padding = new Padding(10, 6, 10, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Build", (_,_) => Build()),
                NewButton("Build + paste handle", (_,_) => BuildAndPaste()),
                _status,
                NewButton("Close", (_,_) => Close()),
            });

            Controls.Add(_tabs);
            Controls.Add(bottom);
        }

        private JObject CurrentSpec()
        {
            var page = _tabs.SelectedTab as CurveTab
                ?? throw new ConvexException("no curve tab selected");
            return page.BuildSpec();
        }

        private void Build()
        {
            try { _status.Text = "OK — " + CxParse.FormatHandle(Cx.BuildCurve(CurrentSpec())); }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private void BuildAndPaste()
        {
            try
            {
                var fmt = CxParse.FormatHandle(Cx.BuildCurve(CurrentSpec()));
                var addr = SheetHelpers.WriteFormulaAtSelection("=\"" + fmt + "\"");
                _status.Text = $"Pasted {fmt} at {addr}";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }

        // ============================================================

        private abstract class CurveTab : TabPage
        {
            public abstract JObject BuildSpec();
        }

        private sealed class DiscreteTab : CurveTab
        {
            private readonly TextBox _name = new();
            private readonly DateTimePicker _refDate = new() { Format = DateTimePickerFormat.Short, Value = DateTime.Today };
            private readonly ComboBox _valueKind = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly ComboBox _interpolation = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly ComboBox _dayCount = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly ComboBox _compounding = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly DataGridView _points = new()
            {
                Anchor = AnchorStyles.Left | AnchorStyles.Right,
                Height = 200,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                AllowUserToResizeRows = false,
                RowHeadersVisible = false,
            };

            public DiscreteTab()
            {
                Text = "Discrete";
                _valueKind.Items.AddRange(new object[] { "zero_rate", "discount_factor" });
                _valueKind.SelectedIndex = 0;
                _interpolation.Items.AddRange(new object[] { "linear", "log_linear", "cubic_spline", "monotone_convex" });
                _interpolation.SelectedIndex = 0;
                _dayCount.Items.AddRange(new object[] { "Act360", "Act365Fixed", "ActActIsda", "ActActIcma", "Thirty360US", "Thirty360E" });
                _dayCount.SelectedItem = "Act365Fixed";
                _compounding.Items.AddRange(new object[] { "Annual", "SemiAnnual", "Quarterly", "Monthly", "Continuous", "Simple" });
                _compounding.SelectedItem = "Continuous";
                _points.Columns.Add("tenor", "Tenor (years)");
                _points.Columns.Add("value", "Value (rate as decimal, or DF)");

                var g = new TableLayoutPanel
                {
                    Dock = DockStyle.Fill,
                    ColumnCount = 2,
                    RowCount = 7,
                    Padding = new Padding(10),
                };
                g.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
                g.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
                for (int i = 0; i < 7; i++) g.RowStyles.Add(new RowStyle(SizeType.AutoSize));
                AddRow(g, 0, "Name (optional):", _name);
                AddRow(g, 1, "Reference date:", _refDate);
                AddRow(g, 2, "Value kind:", _valueKind);
                AddRow(g, 3, "Interpolation:", _interpolation);
                AddRow(g, 4, "Day count:", _dayCount);
                AddRow(g, 5, "Compounding:", _compounding);
                AddRow(g, 6, "Points:", _points);
                Controls.Add(g);
            }

            public override JObject BuildSpec()
            {
                var tenors = new JArray();
                var values = new JArray();
                foreach (DataGridViewRow row in _points.Rows)
                {
                    if (row.IsNewRow) continue;
                    var t = (row.Cells[0].Value ?? "").ToString()!.Trim();
                    var v = (row.Cells[1].Value ?? "").ToString()!.Trim();
                    if (t.Length == 0 || v.Length == 0) continue;
                    if (!double.TryParse(t, NumberStyles.Any, CultureInfo.InvariantCulture, out var td))
                        throw new ConvexException("invalid tenor " + t);
                    if (!double.TryParse(v, NumberStyles.Any, CultureInfo.InvariantCulture, out var vd))
                        throw new ConvexException("invalid value " + v);
                    tenors.Add(td);
                    values.Add(vd);
                }
                if (tenors.Count < 2) throw new ConvexException("at least two points required");
                return Discrete(
                    _name.Text, _refDate.Value, tenors, values,
                    (string)_valueKind.SelectedItem!,
                    (string)_interpolation.SelectedItem!,
                    (string)_dayCount.SelectedItem!,
                    (string)_compounding.SelectedItem!);
            }
        }

        private sealed class BootstrapTab : CurveTab
        {
            private readonly TextBox _name = new();
            private readonly DateTimePicker _refDate = new() { Format = DateTimePickerFormat.Short, Value = DateTime.Today };
            private readonly ComboBox _method = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly ComboBox _interpolation = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly ComboBox _dayCount = new() { DropDownStyle = ComboBoxStyle.DropDownList };
            private readonly DataGridView _instruments = new()
            {
                Anchor = AnchorStyles.Left | AnchorStyles.Right,
                Height = 220,
                AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
                AllowUserToResizeRows = false,
                RowHeadersVisible = false,
            };

            public BootstrapTab()
            {
                Text = "Bootstrap";
                _method.Items.AddRange(new object[] { "global_fit", "piecewise" });
                _method.SelectedIndex = 0;
                _interpolation.Items.AddRange(new object[] { "linear", "log_linear", "cubic_spline", "monotone_convex" });
                _interpolation.SelectedIndex = 0;
                _dayCount.Items.AddRange(new object[] { "Act360", "Act365Fixed", "ActActIsda", "ActActIcma", "Thirty360US", "Thirty360E" });
                _dayCount.SelectedItem = "Act360";

                var kindCol = new DataGridViewComboBoxColumn
                {
                    HeaderText = "Kind",
                    Items = { "deposit", "fra", "swap", "ois" },
                    DataPropertyName = "kind",
                };
                _instruments.Columns.Add(kindCol);
                _instruments.Columns.Add("tenor", "Tenor (years)");
                _instruments.Columns.Add("rate", "Rate (decimal)");

                var g = new TableLayoutPanel
                {
                    Dock = DockStyle.Fill,
                    ColumnCount = 2,
                    RowCount = 6,
                    Padding = new Padding(10),
                };
                g.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
                g.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
                for (int i = 0; i < 6; i++) g.RowStyles.Add(new RowStyle(SizeType.AutoSize));
                AddRow(g, 0, "Name (optional):", _name);
                AddRow(g, 1, "Reference date:", _refDate);
                AddRow(g, 2, "Method:", _method);
                AddRow(g, 3, "Interpolation:", _interpolation);
                AddRow(g, 4, "Day count:", _dayCount);
                AddRow(g, 5, "Instruments:", _instruments);
                Controls.Add(g);
            }

            public override JObject BuildSpec()
            {
                var instruments = new JArray();
                foreach (DataGridViewRow row in _instruments.Rows)
                {
                    if (row.IsNewRow) continue;
                    var kind = (row.Cells[0].Value ?? "").ToString()!.Trim();
                    var tenor = (row.Cells[1].Value ?? "").ToString()!.Trim();
                    var rate = (row.Cells[2].Value ?? "").ToString()!.Trim();
                    if (kind.Length == 0 || tenor.Length == 0 || rate.Length == 0) continue;
                    if (!double.TryParse(tenor, NumberStyles.Any, CultureInfo.InvariantCulture, out var t))
                        throw new ConvexException("invalid tenor " + tenor);
                    if (!double.TryParse(rate, NumberStyles.Any, CultureInfo.InvariantCulture, out var r))
                        throw new ConvexException("invalid rate " + rate);
                    instruments.Add(new JObject { ["kind"] = kind, ["tenor"] = t, ["rate"] = r });
                }
                if (instruments.Count == 0) throw new ConvexException("add at least one instrument");
                return Bootstrap(
                    _name.Text, _refDate.Value,
                    (string)_method.SelectedItem!,
                    instruments,
                    (string)_interpolation.SelectedItem!,
                    (string)_dayCount.SelectedItem!);
            }
        }

        private static void AddRow(TableLayoutPanel grid, int row, string label, Control control)
        {
            grid.Controls.Add(
                new Label { Text = label, Anchor = AnchorStyles.Left, AutoSize = true, Padding = new Padding(0, 6, 0, 0) },
                0, row);
            control.Anchor = AnchorStyles.Left | AnchorStyles.Right;
            grid.Controls.Add(control, 1, row);
        }
    }
}
