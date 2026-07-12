using System;
using System.Drawing;
using System.Globalization;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;
using static Convex.Excel.Helpers.FormUi;

namespace Convex.Excel.Forms
{
    // Scenario ladders: curve scenarios via convex_scenario (the =CX.SCENARIO
    // verb), or a plain yield-shift ladder when no curve exists.
    internal sealed class ScenarioForm : Form
    {
        private readonly ComboBox _bond = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly ComboBox _curve = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly ComboBox _kind = new() { DropDownStyle = ComboBoxStyle.DropDownList };
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

            _result.Columns.Add("name", "Scenario");
            _result.Columns.Add("yield", "Yield (%)");
            _result.Columns.Add("clean", "Clean");
            _result.Columns.Add("dirty", "Dirty");
            _result.Columns.Add("dpnl", "ΔP (clean)");

            _kind.Items.AddRange(new object[] { "parallel", "steepener", "flattener", "credit" });
            _kind.SelectedIndex = 0;

            var inputs = new TableLayoutPanel
            {
                Dock = DockStyle.Top, Height = 236,
                ColumnCount = 2, RowCount = 6, Padding = new Padding(10),
            };
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 130));
            inputs.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 6; i++) inputs.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            AddRow(inputs, 0, "Bond:", _bond);
            AddRow(inputs, 1, "Curve:", _curve);
            AddRow(inputs, 2, "Settlement:", _settle);
            AddRow(inputs, 3, "Base mark:", _baseMark);
            AddRow(inputs, 4, "Shift kind:", _kind);
            AddRow(inputs, 5, "Shifts (bps, csv):", _shifts);

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

        private const string NoCurve = "(none — parallel yield shift)";

        private void ReloadBonds()
        {
            try
            {
                var entries = Cx.ListObjects();
                ComboReload.Reload(_bond, entries
                    .Where(o => o.Kind != "curve")
                    .OrderBy(o => o.Handle)
                    .Select(e => (object)Format(e))
                    .ToArray());
                ComboReload.Reload(_curve, new object[] { NoCurve }
                    .Concat(entries
                        .Where(o => o.Kind == "curve")
                        .OrderBy(o => o.Handle)
                        .Select(e => (object)Format(e)))
                    .ToArray());
                _status.Text = $"{_bond.Items.Count} bond(s), {_curve.Items.Count - 1} curve(s)";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private static string Format(Cx.ObjectEntry e) =>
            e.Name is { Length: > 0 }
                ? $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}  ·  {e.Name}"
                : $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Kind}";

        private void Run()
        {
            try
            {
                _result.Rows.Clear();
                if (_curve.SelectedIndex <= 0)
                    RunYieldLadder();
                else
                    RunCurveScenarios();
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; _result.Rows.Clear(); }
        }

        // Bump the mark-implied YTM and reprice; only parallel is meaningful.
        private void RunYieldLadder()
        {
            var kind = (string)_kind.SelectedItem!;
            if (kind != "parallel")
                throw new ConvexException($"{kind} scenarios need a curve — pick one, or use parallel");

            var handle = ComboReload.HandleOf(_bond, "bond");
            var settle = CxParse.AsIsoDate(_settle.Value.Date);
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

            foreach (var shiftBps in ParseShifts(_shifts.Text))
            {
                var bumpedYield = baseYtm + shiftBps / 10_000.0;
                var markText = (bumpedYield * 100.0).ToString("F8", CultureInfo.InvariantCulture) + "%@SA";
                var r = Cx.Price(new JObject
                {
                    ["bond"] = handle,
                    ["settlement"] = settle,
                    ["mark"] = new JValue(markText),
                    ["quote_frequency"] = "SemiAnnual",
                });
                var clean = (double)r["clean_price"]!;
                AddRow($"{(shiftBps >= 0 ? "+" : "")}{shiftBps:0.#}bp yield",
                    bumpedYield * 100.0, clean, (double)r["dirty_price"]!, clean - baseClean);
            }
            _status.Text = "OK — base YTM " + (baseYtm * 100.0).ToString("F4", CultureInfo.InvariantCulture) + "%";
        }

        private void RunCurveScenarios()
        {
            var req = new JObject
            {
                ["bond"] = ComboReload.HandleOf(_bond, "bond"),
                ["curve"] = ComboReload.HandleOf(_curve, "curve"),
                ["settlement"] = CxParse.AsIsoDate(_settle.Value.Date),
                ["mark"] = new JValue(_baseMark.Text.Trim()),
                ["scenarios"] = Functions.BuildScenarioLadder(
                    ParseShifts(_shifts.Text), (string)_kind.SelectedItem!, 5.0),
                ["quote_frequency"] = "SemiAnnual",
            };

            var result = Cx.Scenario(req);
            foreach (var row in result["rows"] as JArray ?? new JArray())
            {
                AddRow((string?)row["name"] ?? "",
                    ((double?)row["ytm_decimal"] ?? double.NaN) * 100.0,
                    (double?)row["clean_price"] ?? double.NaN,
                    (double?)row["dirty_price"] ?? double.NaN,
                    (double?)row["delta_clean"] ?? double.NaN);
            }
            var baseYtm = ((double?)result["base_ytm_decimal"] ?? double.NaN) * 100.0;
            var z = (double?)result["z_spread_bps"] ?? double.NaN;
            _status.Text = "OK — base YTM " + baseYtm.ToString("F4", CultureInfo.InvariantCulture) +
                           "%, Z " + z.ToString("F1", CultureInfo.InvariantCulture) + "bp";
        }

        private void AddRow(string name, double ytmPct, double clean, double dirty, double delta) =>
            _result.Rows.Add(
                name,
                ytmPct.ToString("F4", CultureInfo.InvariantCulture),
                clean.ToString("F6", CultureInfo.InvariantCulture),
                dirty.ToString("F6", CultureInfo.InvariantCulture),
                delta.ToString("F6", CultureInfo.InvariantCulture));

        private void StampToSheet()
        {
            try
            {
                if (_result.Rows.Count == 0)
                {
                    Run();
                    if (_result.Rows.Count == 0) return; // Run() failed; keep its status
                }
                var rows = _result.Rows.Cast<DataGridViewRow>().Where(r => !r.IsNewRow).ToList();
                var grid = new object[rows.Count + 1, 5];
                grid[0, 0] = "Scenario";
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
    }
}
