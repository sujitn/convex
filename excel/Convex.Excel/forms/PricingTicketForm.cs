using System;
using System.Drawing;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;

namespace Convex.Excel.Forms
{
    // The pricing ticket: pick a bond, type a mark, pick a curve (optional),
    // see clean / dirty / accrued / YTM / Z-spread live. Every recompute goes
    // through convex_price exactly like =CX.PRICE, so the ticket and the cell
    // always agree.
    //
    // "Stamp to sheet" writes both the result grid and the equivalent
    // =CX.PRICE(...) formula so the user can audit and edit from cells.
    internal sealed class PricingTicketForm : Form
    {
        private readonly ComboBox _bond = NewBondCombo();
        private readonly ComboBox _curve = NewCurveCombo();
        private readonly DateTimePicker _settle = new() { Format = DateTimePickerFormat.Short };
        private readonly TextBox _mark = new();
        private readonly ComboBox _frequency = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly DataGridView _result = new()
        {
            ReadOnly = true,
            AllowUserToAddRows = false,
            AllowUserToDeleteRows = false,
            AllowUserToResizeRows = false,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
            RowHeadersVisible = false,
            ColumnHeadersHeight = 22,
        };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public PricingTicketForm()
        {
            Text = "Convex — Pricing Ticket";
            Size = new Size(620, 460);
            MinimumSize = new Size(520, 360);
            StartPosition = FormStartPosition.CenterParent;

            _frequency.Items.AddRange(new object[] { "SemiAnnual", "Annual", "Quarterly", "Monthly" });
            _frequency.SelectedIndex = 0;

            _result.Columns.Add("Field", "Field");
            _result.Columns.Add("Value", "Value");

            var inputs = new TableLayoutPanel
            {
                Dock = DockStyle.Top,
                Height = 200,
                ColumnCount = 2,
                RowCount = 5,
                Padding = new Padding(10),
            };
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 110));
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 5; i++)
                inputs.RowStyles.Add(new RowStyle(SizeType.Absolute, 32));
            inputs.Controls.Add(new Label { Text = "Bond:", Anchor = AnchorStyles.Left }, 0, 0);
            inputs.Controls.Add(_bond, 1, 0);
            inputs.Controls.Add(new Label { Text = "Curve (opt):", Anchor = AnchorStyles.Left }, 0, 1);
            inputs.Controls.Add(_curve, 1, 1);
            inputs.Controls.Add(new Label { Text = "Settlement:", Anchor = AnchorStyles.Left }, 0, 2);
            inputs.Controls.Add(_settle, 1, 2);
            inputs.Controls.Add(
                new Label
                {
                    Text = "Mark:",
                    Anchor = AnchorStyles.Left,
                    AutoSize = true,
                },
                0, 3);
            inputs.Controls.Add(_mark, 1, 3);
            // Hint label below the mark field — replaces .NET 5+ PlaceholderText.
            // (Lives in row 4 of `inputs`, just before the frequency row that follows.)
            inputs.Controls.Add(new Label { Text = "Quote freq:", Anchor = AnchorStyles.Left }, 0, 4);
            inputs.Controls.Add(_frequency, 1, 4);
            _bond.Anchor = _curve.Anchor = _settle.Anchor = _mark.Anchor = _frequency.Anchor =
                AnchorStyles.Left | AnchorStyles.Right;

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom,
                Height = 44,
                Padding = new Padding(10, 6, 10, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Compute", (_,_) => Compute()),
                NewButton("Stamp to sheet", (_,_) => StampToSheet()),
                NewButton("Refresh objects", (_,_) => RefreshObjects()),
                _status,
                NewButton("Close", (_,_) => Close()),
            });

            _result.Dock = DockStyle.Fill;

            Controls.Add(_result);
            Controls.Add(bottom);
            Controls.Add(inputs);

            _settle.Value = DateTime.Today;
            RefreshObjects();
            _mark.Text = "99.5C";
        }

        private static ComboBox NewBondCombo() =>
            new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private static ComboBox NewCurveCombo() =>
            new() { DropDownStyle = ComboBoxStyle.DropDownList };

        private void RefreshObjects()
        {
            try
            {
                var entries = Cx.ListObjects();
                var bonds = entries.Where(e => e.Kind != "curve").OrderBy(e => e.Handle).ToList();
                var curves = entries.Where(e => e.Kind == "curve").OrderBy(e => e.Handle).ToList();
                Reload(_bond, bonds.Select(e => DescribeEntry(e)).ToArray());
                Reload(_curve, new[] { "(none)" }.Concat(curves.Select(e => DescribeEntry(e))).ToArray());
                _curve.SelectedIndex = 0;
                _status.Text = $"{bonds.Count} bond(s), {curves.Count} curve(s)";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        private static void Reload(ComboBox combo, object[] items)
        {
            int prevIndex = combo.SelectedIndex;
            combo.BeginUpdate();
            combo.Items.Clear();
            combo.Items.AddRange(items);
            if (combo.Items.Count > 0)
                combo.SelectedIndex = Math.Min(Math.Max(prevIndex, 0), combo.Items.Count - 1);
            combo.EndUpdate();
        }

        private static string DescribeEntry(Cx.ObjectEntry e) =>
            (e.Name is { Length: > 0 })
                ? $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}  ·  {e.Name}"
                : $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}";

        private static ulong HandleFromCombo(ComboBox combo, string field)
        {
            var text = combo.SelectedItem?.ToString() ?? "";
            int sep = text.IndexOf(' ');
            var token = sep < 0 ? text : text.Substring(0, sep);
            return CxParse.AsHandle(token, field);
        }

        private void Compute()
        {
            try
            {
                var req = BuildRequest();
                var result = Cx.Price(req);
                ShowResult(result);
                _status.Text = "OK";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
                _result.Rows.Clear();
            }
        }

        private JObject BuildRequest()
        {
            if (_bond.SelectedItem is null)
                throw new ConvexException("select a bond");
            var req = new JObject
            {
                ["bond"] = HandleFromCombo(_bond, "bond"),
                ["settlement"] = CxParse.AsIsoDate(_settle.Value.Date),
                ["mark"] = new JValue(_mark.Text.Trim()),
                ["quote_frequency"] = (string)_frequency.SelectedItem!,
            };
            if (_curve.SelectedIndex > 0)
                req["curve"] = HandleFromCombo(_curve, "curve");
            return req;
        }

        private void ShowResult(JToken r)
        {
            _result.Rows.Clear();
            _result.Rows.Add("Clean", Round((double?)r["clean_price"]));
            _result.Rows.Add("Dirty", Round((double?)r["dirty_price"]));
            _result.Rows.Add("Accrued", Round((double?)r["accrued"]));
            _result.Rows.Add("YTM (%)", Round(((double?)r["ytm_decimal"]) * 100.0));
            var z = (double?)r["z_spread_bps"];
            _result.Rows.Add("Z (bps)", z.HasValue ? z.Value.ToString("F4") : "—");
        }

        private static string Round(double? v) =>
            v.HasValue ? v.Value.ToString("F6") : "—";

        private void StampToSheet()
        {
            try
            {
                var req = BuildRequest();
                var result = Cx.Price(req);
                var grid = new object[5, 2]
                {
                    { "Clean",   (double?)result["clean_price"] ?? double.NaN },
                    { "Dirty",   (double?)result["dirty_price"] ?? double.NaN },
                    { "Accrued", (double?)result["accrued"]     ?? double.NaN },
                    { "YTM (%)", ((double?)result["ytm_decimal"] ?? 0.0) * 100.0 },
                    { "Z (bps)", (double?)result["z_spread_bps"] ?? (object)"" },
                };
                var addr = SheetHelpers.WriteGridAtSelection(grid);
                _status.Text = $"Stamped grid at {addr}";
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
    }
}
