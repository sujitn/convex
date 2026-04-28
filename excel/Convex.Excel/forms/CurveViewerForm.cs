using System;
using System.Drawing;
using System.Globalization;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using System.Windows.Forms.DataVisualization.Charting;

namespace Convex.Excel.Forms
{
    // Plots zero rates and 1Y forward rates across a swept tenor grid for a
    // selected curve. Data and chart use the same convex_curve_query RPC the
    // worksheet UDFs use, so the two views never disagree.
    internal sealed class CurveViewerForm : Form
    {
        private readonly ComboBox _curve = new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 260 };
        private readonly NumericUpDown _maxTenor = new()
        {
            Minimum = 1, Maximum = 60, DecimalPlaces = 0, Value = 30,
            Increment = 1, Width = 80,
        };
        private readonly Chart _chart = new() { Dock = DockStyle.Fill };
        private readonly DataGridView _grid = new()
        {
            Dock = DockStyle.Right, Width = 280,
            ReadOnly = true, AllowUserToAddRows = false,
            AllowUserToResizeRows = false, RowHeadersVisible = false,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public CurveViewerForm()
        {
            Text = "Convex — Curve Viewer";
            Size = new Size(960, 540);
            MinimumSize = new Size(720, 420);
            StartPosition = FormStartPosition.CenterParent;

            _grid.Columns.Add("tenor", "Tenor (yrs)");
            _grid.Columns.Add("zero", "Zero (%)");
            _grid.Columns.Add("forward", "1Y Fwd (%)");

            var chartArea = new ChartArea("main")
            {
                AxisX = { Title = "Tenor (yrs)", LabelStyle = { Format = "F1" } },
                AxisY = { Title = "Rate (%)", LabelStyle = { Format = "F2" } },
            };
            _chart.ChartAreas.Add(chartArea);
            _chart.Legends.Add(new Legend("legend") { Docking = Docking.Bottom });

            var top = new FlowLayoutPanel
            {
                Dock = DockStyle.Top,
                Height = 38,
                Padding = new Padding(8, 6, 8, 6),
            };
            top.Controls.Add(new Label { Text = "Curve:", AutoSize = true, Padding = new Padding(0, 5, 4, 0) });
            top.Controls.Add(_curve);
            top.Controls.Add(new Label { Text = "Max tenor:", AutoSize = true, Padding = new Padding(8, 5, 4, 0) });
            top.Controls.Add(_maxTenor);
            top.Controls.Add(NewButton("Refresh", (_, _) => Render()));
            top.Controls.Add(NewButton("Reload", (_, _) => ReloadCurves()));

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 38, Padding = new Padding(8, 6, 8, 6),
            };
            bottom.Controls.Add(_status);
            bottom.Controls.Add(NewButton("Close", (_, _) => Close()));

            Controls.Add(_chart);
            Controls.Add(_grid);
            Controls.Add(top);
            Controls.Add(bottom);

            ReloadCurves();
        }

        private void ReloadCurves()
        {
            try
            {
                _curve.Items.Clear();
                foreach (var e in Cx.ListObjects().Where(o => o.Kind == "curve").OrderBy(o => o.Handle))
                {
                    var label = e.Name is { Length: > 0 }
                        ? $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Name}"
                        : CxParse.FormatHandle(e.Handle);
                    _curve.Items.Add(label);
                }
                if (_curve.Items.Count > 0) _curve.SelectedIndex = 0;
                Render();
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private void Render()
        {
            try
            {
                if (_curve.SelectedItem is null) { _chart.Series.Clear(); _grid.Rows.Clear(); return; }
                var label = _curve.SelectedItem!.ToString()!;
                int sep = label.IndexOf(' ');
                var token = sep < 0 ? label : label.Substring(0, sep);
                var handle = CxParse.AsHandle(token, "curve");

                _chart.Series.Clear();
                var zSeries = new Series("Zero rate")
                {
                    ChartType = SeriesChartType.Line,
                    BorderWidth = 2,
                    Color = Color.FromArgb(0, 120, 215),
                };
                var fSeries = new Series("1Y forward")
                {
                    ChartType = SeriesChartType.Line,
                    BorderWidth = 2,
                    Color = Color.FromArgb(220, 60, 60),
                };
                _grid.Rows.Clear();

                double max = (double)_maxTenor.Value;
                double step = max <= 5 ? 0.25 : (max <= 15 ? 0.5 : 1.0);
                for (double t = step; t <= max + 1e-9; t += step)
                {
                    var zero = Query(handle, "zero", t, null);
                    var fwd = Query(handle, "forward", t, t + 1.0);
                    zSeries.Points.AddXY(t, zero * 100.0);
                    fSeries.Points.AddXY(t, fwd * 100.0);
                    _grid.Rows.Add(t.ToString("F2", CultureInfo.InvariantCulture),
                        (zero * 100.0).ToString("F4", CultureInfo.InvariantCulture),
                        (fwd * 100.0).ToString("F4", CultureInfo.InvariantCulture));
                }
                _chart.Series.Add(zSeries);
                _chart.Series.Add(fSeries);
                _status.Text = "OK";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        private static double Query(ulong handle, string kind, double tenor, double? tenorEnd)
        {
            var req = new JObject
            {
                ["curve"] = handle,
                ["query"] = kind,
                ["tenor"] = tenor,
            };
            if (tenorEnd.HasValue) req["tenor_end"] = tenorEnd.Value;
            var resp = Cx.CurveQuery(req);
            return (double?)resp["value"] ?? throw new ConvexException("missing value");
        }

        private static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }
    }
}
