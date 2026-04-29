using System;
using System.Drawing;
using System.Globalization;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;

namespace Convex.Excel.Forms
{
    // Bump-style scenarios on a bond: parallel shift (bps), KRD across a tenor
    // ladder, or a custom yield-shift list. Each row reprices via convex_price
    // with a yield mark derived from the base YTM plus the shift, so the
    // engine path is identical to =CX.PRICE.
    internal sealed class ScenarioForm : Form
    {
        private readonly ComboBox _bond = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly DateTimePicker _settle = new() { Format = DateTimePickerFormat.Short, Value = DateTime.Today };
        private readonly TextBox _baseMark = new() { Text = "99.5C" };
        private readonly TextBox _shifts = new() { Text = "-50, -25, -10, 0, 10, 25, 50" };
        private readonly DataGridView _result = new()
        {
            Dock = DockStyle.Fill,
            ReadOnly = true, AllowUserToAddRows = false,
            AllowUserToResizeRows = false, RowHeadersVisible = false,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public ScenarioForm()
        {
            Text = "Convex — Scenarios";
            Size = new Size(720, 520);
            MinimumSize = new Size(620, 400);
            StartPosition = FormStartPosition.CenterParent;

            _result.Columns.Add("shift", "Shift (bps)");
            _result.Columns.Add("yield", "Yield (%)");
            _result.Columns.Add("clean", "Clean");
            _result.Columns.Add("dirty", "Dirty");
            _result.Columns.Add("dpnl", "ΔP (clean)");

            var inputs = new TableLayoutPanel
            {
                Dock = DockStyle.Top, Height = 170,
                ColumnCount = 2, RowCount = 4, Padding = new Padding(10),
            };
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 4; i++) inputs.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            AddRow(inputs, 0, "Bond:", _bond);
            AddRow(inputs, 1, "Settlement:", _settle);
            AddRow(inputs, 2, "Base mark:", _baseMark);
            AddRow(inputs, 3, "Shifts (bps, csv):", _shifts);

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(10, 6, 10, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Run", (_,_) => Run()),
                NewButton("Stamp to sheet", (_,_) => StampToSheet()),
                NewButton("Refresh objects", (_,_) => ReloadBonds()),
                _status,
                NewButton("Close", (_,_) => Close()),
            });

            Controls.Add(_result);
            Controls.Add(bottom);
            Controls.Add(inputs);

            ReloadBonds();
        }

        private void ReloadBonds()
        {
            try
            {
                _bond.Items.Clear();
                foreach (var e in Cx.ListObjects().Where(o => o.Kind != "curve").OrderBy(o => o.Handle))
                    _bond.Items.Add(Format(e));
                if (_bond.Items.Count > 0) _bond.SelectedIndex = 0;
                _status.Text = $"{_bond.Items.Count} bond(s)";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static string Format(Cx.ObjectEntry e) =>
            e.Name is { Length: > 0 }
                ? $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}  ·  {e.Name}"
                : $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}";

        private ulong BondHandle()
        {
            if (_bond.SelectedItem is null) throw new ConvexException("select a bond");
            var t = _bond.SelectedItem!.ToString()!;
            int sep = t.IndexOf(' ');
            return CxParse.AsHandle(sep < 0 ? t : t.Substring(0, sep), "bond");
        }

        private void Run()
        {
            try
            {
                var handle = BondHandle();
                var settle = CxParse.AsIsoDate(_settle.Value.Date);

                // 1. Compute base from the user's mark (gives us YTM and clean baseline).
                var baseReq = new JObject
                {
                    ["bond"] = handle,
                    ["settlement"] = settle,
                    ["mark"] = new JValue(_baseMark.Text.Trim()),
                    ["quote_frequency"] = "SemiAnnual",
                };
                var baseResult = Cx.Price(baseReq);
                var baseYtm = (double)baseResult["ytm_decimal"]!;
                var baseClean = (double)baseResult["clean_price"]!;

                _result.Rows.Clear();
                foreach (var shiftBps in ParseShifts(_shifts.Text))
                {
                    var bumpedYield = baseYtm + shiftBps / 10_000.0;
                    var markText = (bumpedYield * 100.0).ToString("F8", CultureInfo.InvariantCulture) + "%@SA";
                    var req = new JObject
                    {
                        ["bond"] = handle,
                        ["settlement"] = settle,
                        ["mark"] = new JValue(markText),
                        ["quote_frequency"] = "SemiAnnual",
                    };
                    var r = Cx.Price(req);
                    var clean = (double)r["clean_price"]!;
                    var dirty = (double)r["dirty_price"]!;
                    _result.Rows.Add(
                        shiftBps.ToString("F1", CultureInfo.InvariantCulture),
                        (bumpedYield * 100.0).ToString("F4", CultureInfo.InvariantCulture),
                        clean.ToString("F6", CultureInfo.InvariantCulture),
                        dirty.ToString("F6", CultureInfo.InvariantCulture),
                        (clean - baseClean).ToString("F6", CultureInfo.InvariantCulture));
                }
                _status.Text = "OK — base YTM " + (baseYtm * 100.0).ToString("F4", CultureInfo.InvariantCulture) + "%";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; _result.Rows.Clear(); }
        }

        private void StampToSheet()
        {
            try
            {
                if (_result.Rows.Count == 0) Run();
                var rows = _result.Rows.Cast<DataGridViewRow>().Where(r => !r.IsNewRow).ToList();
                var grid = new object[rows.Count + 1, 5];
                grid[0, 0] = "Shift (bps)";
                grid[0, 1] = "Yield (%)";
                grid[0, 2] = "Clean";
                grid[0, 3] = "Dirty";
                grid[0, 4] = "ΔP (clean)";
                for (int i = 0; i < rows.Count; i++)
                    for (int j = 0; j < 5; j++)
                        grid[i + 1, j] = rows[i].Cells[j].Value ?? "";
                var addr = SheetHelpers.WriteGridAtSelection(grid);
                _status.Text = "Stamped at " + addr;
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static double[] ParseShifts(string text)
        {
            return text.Split(new[] { ',', ';', ' ', '\t' }, StringSplitOptions.RemoveEmptyEntries)
                .Select(t => double.Parse(t.Trim(), NumberStyles.Any, CultureInfo.InvariantCulture))
                .ToArray();
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
