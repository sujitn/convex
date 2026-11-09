using System;
using System.Collections.Generic;
using System.Drawing;
using System.Globalization;
using System.Linq;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;
using Convex.Excel.Helpers;
using Convex.Excel.Server;
using static Convex.Excel.Helpers.FormUi;

namespace Convex.Excel.Forms
{
    // The pre-trade ticket (Excel plan X4): a book and the trades proposed for
    // it (signed quantities), checked against the book's limits on the server.
    // Check reads the book's status now and POSTs the changes to the check
    // route, which stores nothing (server D88), and shows each limit before
    // and after. "From selection" reads trades from the sheet (identifier,
    // quantity, [rating], [sector]); "Stamp CX.LIMIT" writes live limit cells
    // for the book at the selection.
    internal sealed class LimitTicketForm : Form
    {
        private readonly TextBox _book = new() { Width = 240 };
        private readonly DataGridView _trades = new()
        {
            AllowUserToAddRows = true,
            AllowUserToResizeRows = false,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
            RowHeadersWidth = 28,
            ColumnHeadersHeight = 22,
        };
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
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray, Padding = new Padding(0, 6, 0, 0) };
        private readonly Button _check;
        private CancellationTokenSource? _running;
        private List<LimitTicketRow> _shown = new();

        public LimitTicketForm()
        {
            Text = "Convex — Pre-trade Limit Check";
            Size = new Size(900, 600);
            MinimumSize = new Size(700, 420);

            _trades.Columns.Add("Identifier", "Instrument id, CUSIP or ISIN");
            _trades.Columns.Add("Quantity", "Signed quantity (face)");
            _trades.Columns.Add("Rating", "Rating (new)");
            _trades.Columns.Add("Sector", "Sector (new)");
            _trades.Columns[2].FillWeight = 50;
            _trades.Columns[3].FillWeight = 50;

            foreach (var (name, header) in new[]
                     {
                         ("Limit", "Limit"), ("Measure", "Measure"), ("Bounds", "Bounds"),
                         ("NowStatus", "Now"), ("NowValue", "Value"), ("NowUtil", "Util."),
                         ("AfterStatus", "After trade"), ("AfterValue", "Value"), ("AfterUtil", "Util."),
                     })
                _result.Columns.Add(name, header);

            var top = new FlowLayoutPanel { Dock = DockStyle.Top, Height = 40, Padding = new Padding(10, 8, 10, 0) };
            top.Controls.Add(new Label { Text = "Book:", AutoSize = true, Padding = new Padding(0, 6, 0, 0) });
            top.Controls.Add(_book);
            top.Controls.Add(new Label
            {
                Text = "Quantities are trades: each is added to the held position.",
                AutoSize = true, ForeColor = Color.Gray, Padding = new Padding(12, 6, 0, 0),
            });

            _check = NewButton("Check", (_, _) => Check());
            var buttons = new FlowLayoutPanel { Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(10, 6, 10, 6) };
            buttons.Controls.AddRange(new Control[]
            {
                _check,
                NewButton("From selection", (_, _) => FromSelection()),
                NewButton("Stamp CX.LIMIT", (_, _) => Stamp()),
                NewButton("Clear", (_, _) => { _trades.Rows.Clear(); _result.Rows.Clear(); _status.Text = ""; }),
                NewButton("Close", (_, _) => Close()),
                _status,
            });

            _trades.Dock = DockStyle.Fill;
            _result.Dock = DockStyle.Fill;
            var split = new SplitContainer { Dock = DockStyle.Fill, Orientation = Orientation.Horizontal, SplitterDistance = 170 };
            split.Panel1.Controls.Add(_trades);
            split.Panel2.Controls.Add(_result);

            Controls.Add(split);
            Controls.Add(buttons);
            Controls.Add(top);
            FormClosed += (_, _) => _running?.Cancel();
        }

        private void Check()
        {
            var book = _book.Text.Trim();
            List<(int GridRow, LimitChangeRow Change)> rows;
            try
            {
                LimitFields.ValidateId(book, "book");
                rows = ReadTrades();
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
                return;
            }
            var endpoint = ServerLink.Source.HttpEndpoint();
            if (endpoint == null)
            {
                _status.Text = "Not connected: set the server (Server on the ribbon) and sign in.";
                return;
            }
            _running?.Cancel();
            var cts = _running = new CancellationTokenSource();
            _check.Enabled = false;
            _status.Text = "Checking…";
            var (baseUri, token) = endpoint.Value;
            var changes = rows.Select(r => r.Change).ToList();
            Task.Run(async () =>
            {
                // Both read the same accepted results; they are not one read.
                var now = LimitRoutes.StatusAsync(ServerLink.SharedHttp, baseUri, token, book, cts.Token);
                var after = LimitRoutes.CheckAsync(ServerLink.SharedHttp, baseUri, token, book, changes, cts.Token);
                return (await now.ConfigureAwait(false), await after.ConfigureAwait(false));
            }).ContinueWith(t =>
            {
                if (IsDisposed || cts.IsCancellationRequested) return;
                _check.Enabled = true;
                if (t.IsFaulted || t.IsCanceled)
                {
                    _status.Text = "ERROR: " + (t.Exception?.InnerException?.Message ?? "the check did not finish");
                    return;
                }
                ShowResult(book, rows, t.Result.Item1, t.Result.Item2);
            }, TaskScheduler.FromCurrentSynchronizationContext());
        }

        private void ShowResult(string book, List<(int GridRow, LimitChangeRow Change)> rows, LimitAnswer now, LimitAnswer after)
        {
            foreach (DataGridViewRow r in _trades.Rows) r.ErrorText = "";
            _result.Rows.Clear();
            if (after.Status == HttpStatusCode.Unauthorized || now.Status == HttpStatusCode.Unauthorized)
                ServerLink.Source.Tokens.Rejected();
            if (after.Read == null)
            {
                foreach (var (index, reason) in after.Rows)
                    if (index >= 0 && index < rows.Count) _trades.Rows[rows[index].GridRow].ErrorText = reason;
                _status.Text = "REFUSED: " + after.Problem + (after.Rows.Count > 0 ? " (see the marked rows)" : "");
                return;
            }
            _shown = LimitTicket.Compare(now.Read, after.Read);
            foreach (var row in _shown)
            {
                int i = _result.Rows.Add(row.LimitId, row.Measure, row.Bounds,
                    row.Now?.Status ?? "", LimitTicket.Number(row.Now?.Value), LimitTicket.Percent(row.Now?.Utilisation),
                    row.After?.Status ?? "(gone)", LimitTicket.Number(row.After?.Value), LimitTicket.Percent(row.After?.Utilisation));
                var cells = _result.Rows[i].Cells;
                cells["NowStatus"].Style.BackColor = Colour(row.Now?.Status);
                cells["AfterStatus"].Style.BackColor = Colour(row.After?.Status);
                if (row.Worse) cells["AfterStatus"].Style.Font = new Font(_result.Font, FontStyle.Bold);
                var reason = row.After?.Reason ?? row.Now?.Reason;
                if (reason != null) cells["AfterStatus"].ToolTipText = reason;
            }
            int breaches = _shown.Count(r => r.After?.Status == "breach");
            int worse = _shown.Count(r => r.Worse);
            _status.Text =
                $"Book {book} v{after.Read.BookVersion.ToString(CultureInfo.InvariantCulture)}, limit set " +
                $"v{after.Read.LimitSetVersion.ToString(CultureInfo.InvariantCulture)}, checked " +
                $"{after.Read.EvaluatedAtUtc.ToLocalTime():T}: {_shown.Count} limits, {breaches} in breach" +
                (rows.Count == 0 ? " now (no trades entered)" : " after the trade") +
                (worse > 0 ? $", {worse} worse" : "") +
                (now.Read == null ? $" (current status: {now.Problem})" : "") +
                (after.Read.Limits.Any(l => l.Demo) ? " — demo data" : "");
        }

        private static Color Colour(string? status) => status switch
        {
            "breach" => Color.FromArgb(255, 199, 206),
            "warning" => Color.FromArgb(255, 235, 156),
            "unknown" => Color.FromArgb(217, 217, 217),
            "within" => Color.FromArgb(198, 239, 206),
            _ => Color.Empty,
        };

        /// The filled trade rows; each blank row is skipped.
        private List<(int GridRow, LimitChangeRow Change)> ReadTrades()
        {
            var rows = new List<(int, LimitChangeRow)>();
            foreach (DataGridViewRow r in _trades.Rows)
            {
                if (r.IsNewRow) continue;
                var id = Convert.ToString(r.Cells[0].Value, CultureInfo.InvariantCulture)?.Trim() ?? "";
                var quantity = r.Cells[1].Value;
                if (id.Length == 0 && string.IsNullOrWhiteSpace(Convert.ToString(quantity, CultureInfo.InvariantCulture))) continue;
                try
                {
                    Identifiers.Classify(id);
                    rows.Add((r.Index, new LimitChangeRow
                    {
                        Identifier = id,
                        Quantity = LimitChangeRow.ParseQuantity(quantity),
                        Rating = Convert.ToString(r.Cells[2].Value, CultureInfo.InvariantCulture),
                        Sector = Convert.ToString(r.Cells[3].Value, CultureInfo.InvariantCulture),
                    }));
                }
                catch (ConvexException ex)
                {
                    throw new ConvexException($"row {r.Index + 1}: {ex.Message}");
                }
            }
            if (rows.Count > 256) throw new ConvexException("a check takes at most 256 trades");
            return rows;
        }

        /// Trades from the selected cells: identifier, quantity, then an
        /// optional rating and sector. A first row whose quantity is not a
        /// number is taken as a header.
        private void FromSelection()
        {
            try
            {
                var value = SheetHelpers.ReadSelection();
                if (value is not object[,] grid || grid.GetLength(1) < 2)
                    throw new ConvexException("select two to four columns: identifier, quantity, [rating], [sector]");
                _trades.Rows.Clear();
                int r0 = grid.GetLowerBound(0), c0 = grid.GetLowerBound(1);
                int columns = Math.Min(4, grid.GetLength(1));
                for (int r = r0; r <= grid.GetUpperBound(0); r++)
                {
                    var quantity = grid[r, c0 + 1];
                    if (r == r0 && quantity is string s && !decimal.TryParse(s.Replace(",", ""), NumberStyles.Float, CultureInfo.InvariantCulture, out _))
                        continue;
                    if (grid[r, c0] == null && quantity == null) continue;
                    var cells = new object?[4];
                    for (int c = 0; c < columns; c++)
                        cells[c] = grid[r, c0 + c] is double d && c == 1
                            ? d.ToString("R", CultureInfo.InvariantCulture)
                            : grid[r, c0 + c];
                    _trades.Rows.Add(cells);
                }
                _status.Text = $"{_trades.Rows.Count - 1} trades read from the selection";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        /// Live limit cells for the book at the selection: one row per limit
        /// of the last check (or of a fresh status read), with its status,
        /// value and utilisation.
        private void Stamp()
        {
            var book = _book.Text.Trim();
            try
            {
                LimitFields.ValidateId(book, "book");
                if (_shown.Count == 0) throw new ConvexException("check the book first, so its limits are known");
                var grid = new object[_shown.Count + 1, 5];
                var header = new[] { "Limit", "Status", "Value", "Utilisation", "Evaluated (UTC)" };
                for (int c = 0; c < 5; c++) grid[0, c] = header[c];
                var quoted = "\"" + book.Replace("\"", "\"\"") + "\"";
                for (int i = 0; i < _shown.Count; i++)
                {
                    var id = _shown[i].LimitId;
                    grid[i + 1, 0] = id;
                    string Formula(string field) => $"=CX.LIMIT({quoted}, \"{id}\", \"{field}\")";
                    grid[i + 1, 1] = Formula("status");
                    grid[i + 1, 2] = Formula("value");
                    grid[i + 1, 3] = Formula("utilisation");
                    grid[i + 1, 4] = Formula("evaluated_at");
                }
                var address = SheetHelpers.WriteFormulasAtSelection(grid, new[] { null, null, "#,##0.00", "0.0%", "yyyy-mm-dd hh:mm:ss" });
                _status.Text = $"Stamped {_shown.Count} limits of {book} at {address}";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }
    }
}
