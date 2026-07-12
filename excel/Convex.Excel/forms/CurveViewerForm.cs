using System;
using System.Drawing;
using System.Globalization;
using System.Linq;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System.Windows.Forms;
using System.Windows.Forms.DataVisualization.Charting;
using static Convex.Excel.Helpers.FormUi;

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

            // After handle creation — the async render marshals back with
            // BeginInvoke, which needs a live window handle.
            Load += (_, _) => ReloadCurves();
        }

        private void ReloadCurves()
        {
            try
            {
                var items = Cx.ListObjects()
                    .Where(o => o.Kind == "curve")
                    .OrderBy(o => o.Handle)
                    .Select(e => (object)(e.Name is { Length: > 0 }
                        ? $"{CxParse.FormatHandle(e.Handle)}  ·  {e.Name}"
                        : CxParse.FormatHandle(e.Handle)))
                    .ToArray();
                Convex.Excel.Helpers.ComboReload.Reload(_curve, items);
                Render();
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        // Monotonic render id so a slow background sweep can't paint over a
        // newer selection.
        private int _renderSeq;

        private void Render()
        {
            try
            {
                if (_curve.SelectedItem is null) { _chart.Series.Clear(); _grid.Rows.Clear(); return; }
                var handle = Convex.Excel.Helpers.ComboReload.HandleOf(_curve, "curve");

                double max = (double)_maxTenor.Value;
                double step = max <= 5 ? 0.25 : (max <= 15 ? 0.5 : 1.0);
                int seq = ++_renderSeq;
                _status.Text = "computing…";

                // The sweep is ~2 FFI calls per point — run it off the UI
                // thread so a modeless viewer never freezes Excel's message
                // pump, then marshal the points back.
                System.Threading.Tasks.Task.Run(() =>
                {
                    var points = new System.Collections.Generic.List<(double t, double zero, double fwd)>();
                    for (double t = step; t <= max + 1e-9; t += step)
                    {
                        var zero = Query(handle, "zero", t, null);
                        var fwd = Query(handle, "forward", t, t + 1.0);
                        points.Add((t, zero, fwd));
                    }
                    return points;
                }).ContinueWith(task =>
                {
                    if (IsDisposed || seq != _renderSeq) return;
                    BeginInvoke(new Action(() =>
                    {
                        if (seq != _renderSeq) return;
                        try
                        {
                            if (task.IsFaulted)
                            {
                                _status.Text = "ERROR: " +
                                    (task.Exception?.GetBaseException().Message ?? "sweep failed");
                                return;
                            }
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
                            foreach (var (t, zero, fwd) in task.Result)
                            {
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
                    }));
                });
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
    }
}
