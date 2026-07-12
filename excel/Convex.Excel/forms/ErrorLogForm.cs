using System;
using System.Drawing;
using System.Windows.Forms;
using ExcelDna.Integration;
using static Convex.Excel.Helpers.FormUi;

namespace Convex.Excel.Forms
{
    // Read-only view over CxErrorStore: every CX.* error recorded this
    // session, newest first. Double-click a row to jump to the failing cell.
    // Entries persist after a cell is fixed (success paths don't touch the
    // store) — the timestamp column makes stale entries recognizable.
    internal sealed class ErrorLogForm : Form
    {
        private readonly DataGridView _grid = new()
        {
            Dock = DockStyle.Fill,
            ReadOnly = true,
            AllowUserToAddRows = false,
            AllowUserToResizeRows = false,
            RowHeadersVisible = false,
            SelectionMode = DataGridViewSelectionMode.FullRowSelect,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public ErrorLogForm()
        {
            Text = "Convex — Error Log";
            Size = new Size(760, 420);
            MinimumSize = new Size(560, 300);

            _grid.Columns.Add("time", "Time (UTC)");
            _grid.Columns.Add("cell", "Cell");
            _grid.Columns.Add("code", "Code");
            _grid.Columns.Add("message", "Message");
            _grid.Columns[0].FillWeight = 16;
            _grid.Columns[1].FillWeight = 16;
            _grid.Columns[2].FillWeight = 14;
            _grid.Columns[3].FillWeight = 54;
            _grid.CellDoubleClick += (_, e) => { if (e.RowIndex >= 0) GoTo(e.RowIndex); };

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(8, 6, 8, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Refresh", (_, _) => Reload()),
                NewButton("Clear log", (_, _) => { CxErrorStore.Clear(); Reload(); }),
                _status,
                NewButton("Close", (_, _) => Close()),
            });

            Controls.Add(_grid);
            Controls.Add(bottom);

            Reload();
        }

        private void Reload()
        {
            _grid.Rows.Clear();
            var entries = CxErrorStore.Snapshot();
            foreach (var d in entries)
                _grid.Rows.Add(d.Utc.ToString("HH:mm:ss"), d.Address, d.Code, d.Message);
            _status.Text = $"{entries.Count} error(s) recorded — double-click to jump to the cell";
        }

        private void GoTo(int rowIndex)
        {
            var address = (string?)_grid.Rows[rowIndex].Cells[1].Value;
            if (string.IsNullOrEmpty(address) || address == "(non-cell)" || !address!.Contains("!"))
                return;
            try
            {
                ((dynamic)ExcelDnaUtil.Application).Goto("'" + address.Replace("!", "'!"), true);
            }
            catch
            {
                // Address may be a stable key (worker-thread capture) that
                // Goto can't parse — nothing to jump to.
            }
        }
    }
}
