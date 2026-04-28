using System;
using System.Drawing;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;

namespace Convex.Excel.Forms
{
    // Spread ticket: bond × curve × mark × spread family → grid of bps and
    // related sensitivities. Routes through convex_spread, identical to
    // =CX.SPREAD(...).
    internal sealed class SpreadTicketForm : Form
    {
        private readonly ComboBox _bond = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly ComboBox _curve = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly DateTimePicker _settle = new() { Format = DateTimePickerFormat.Short, Value = DateTime.Today };
        private readonly TextBox _mark = new() { Text = "99.5C" };
        private readonly ComboBox _spreadType = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly TextBox _vol = new() { Text = "1.0" };
        private readonly DataGridView _result = new()
        {
            Dock = DockStyle.Fill,
            ReadOnly = true, AllowUserToAddRows = false,
            AllowUserToResizeRows = false, RowHeadersVisible = false,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public SpreadTicketForm()
        {
            Text = "Convex — Spread Ticket";
            Size = new Size(640, 460);
            MinimumSize = new Size(540, 380);
            StartPosition = FormStartPosition.CenterParent;

            _spreadType.Items.AddRange(new object[] { "Z", "G", "I", "OAS", "DM", "ASW_PAR", "ASW_PROC" });
            _spreadType.SelectedIndex = 0;
            _result.Columns.Add("field", "Field");
            _result.Columns.Add("value", "Value");

            var inputs = new TableLayoutPanel
            {
                Dock = DockStyle.Top, Height = 230,
                ColumnCount = 2, RowCount = 6, Padding = new Padding(10),
            };
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 6; i++) inputs.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            AddRow(inputs, 0, "Bond:", _bond);
            AddRow(inputs, 1, "Curve:", _curve);
            AddRow(inputs, 2, "Settlement:", _settle);
            AddRow(inputs, 3, "Mark:", _mark);
            AddRow(inputs, 4, "Spread:", _spreadType);
            AddRow(inputs, 5, "Volatility (% — OAS only):", _vol);

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(10, 6, 10, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Compute", (_,_) => Compute()),
                NewButton("Stamp to sheet", (_,_) => StampToSheet()),
                NewButton("Refresh objects", (_,_) => ReloadObjects()),
                _status,
                NewButton("Close", (_,_) => Close()),
            });

            Controls.Add(_result);
            Controls.Add(bottom);
            Controls.Add(inputs);

            ReloadObjects();
        }

        private void ReloadObjects()
        {
            try
            {
                var entries = Cx.ListObjects();
                _bond.Items.Clear();
                foreach (var e in entries.Where(o => o.Kind != "curve").OrderBy(o => o.Handle))
                    _bond.Items.Add(Format(e));
                _curve.Items.Clear();
                foreach (var e in entries.Where(o => o.Kind == "curve").OrderBy(o => o.Handle))
                    _curve.Items.Add(Format(e));
                if (_bond.Items.Count > 0) _bond.SelectedIndex = 0;
                if (_curve.Items.Count > 0) _curve.SelectedIndex = 0;
                _status.Text = $"{_bond.Items.Count} bond(s), {_curve.Items.Count} curve(s)";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static string Format(Cx.ObjectEntry e) =>
            e.Name is { Length: > 0 }
                ? $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}  ·  {e.Name}"
                : $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}";

        private static ulong HandleFromCombo(ComboBox c, string field)
        {
            var text = c.SelectedItem?.ToString() ?? "";
            int sep = text.IndexOf(' ');
            return CxParse.AsHandle(sep < 0 ? text : text.Substring(0, sep), field);
        }

        private JObject BuildRequest()
        {
            if (_bond.SelectedItem is null) throw new ConvexException("select a bond");
            if (_curve.SelectedItem is null) throw new ConvexException("select a curve");
            var req = new JObject
            {
                ["bond"] = HandleFromCombo(_bond, "bond"),
                ["curve"] = HandleFromCombo(_curve, "curve"),
                ["settlement"] = CxParse.AsIsoDate(_settle.Value.Date),
                ["mark"] = new JValue(_mark.Text.Trim()),
                ["spread_type"] = CxParse.AsSpreadType((string)_spreadType.SelectedItem!),
            };
            if (((string)_spreadType.SelectedItem!) == "OAS"
                && double.TryParse(_vol.Text.Trim(), System.Globalization.NumberStyles.Any,
                    System.Globalization.CultureInfo.InvariantCulture, out var vp))
            {
                req["params"] = new JObject { ["volatility"] = vp / 100.0 };
            }
            return req;
        }

        private void Compute()
        {
            try
            {
                var r = Cx.Spread(BuildRequest());
                _result.Rows.Clear();
                Add("Spread (bps)", (double?)r["spread_bps"]);
                Add("Spread DV01", (double?)r["spread_dv01"]);
                Add("Spread Duration", (double?)r["spread_duration"]);
                Add("Option Value", (double?)r["option_value"]);
                Add("Effective Duration", (double?)r["effective_duration"]);
                Add("Effective Convexity", (double?)r["effective_convexity"]);
                _status.Text = "OK";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; _result.Rows.Clear(); }
        }

        private void Add(string field, double? value)
        {
            _result.Rows.Add(field, value.HasValue ? value.Value.ToString("F6") : "—");
        }

        private void StampToSheet()
        {
            try
            {
                var r = Cx.Spread(BuildRequest());
                var rows = new System.Collections.Generic.List<(string, object)>
                {
                    ("Spread (bps)", (double?)r["spread_bps"] ?? double.NaN),
                };
                void AddIf(string label, string key)
                {
                    var v = (double?)r[key]; if (v.HasValue) rows.Add((label, v.Value));
                }
                AddIf("Spread DV01", "spread_dv01");
                AddIf("Spread Duration", "spread_duration");
                AddIf("Option Value", "option_value");
                AddIf("Effective Duration", "effective_duration");
                AddIf("Effective Convexity", "effective_convexity");
                var grid = new object[rows.Count, 2];
                for (int i = 0; i < rows.Count; i++) { grid[i, 0] = rows[i].Item1; grid[i, 1] = rows[i].Item2; }
                var addr = SheetHelpers.WriteGridAtSelection(grid);
                _status.Text = $"Stamped at {addr}";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static void AddRow(TableLayoutPanel grid, int row, string label, Control control)
        {
            grid.Controls.Add(
                new Label { Text = label, Anchor = AnchorStyles.Left, AutoSize = true, Padding = new Padding(0, 6, 0, 0) }, 0, row);
            control.Anchor = AnchorStyles.Left | AnchorStyles.Right;
            grid.Controls.Add(control, 1, row);
        }

        private static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }
    }
}
