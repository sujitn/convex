using System;
using System.Drawing;
using System.Windows.Forms;
using Convex.Excel.Ops;
using ExcelDna.Integration;
using static Convex.Excel.Helpers.FormUi;

namespace Convex.Excel.Forms
{
    // CX.* errors recorded this session, newest first; double-click jumps to
    // the cell. Entries persist after a cell is fixed — check the timestamp.
    // A second tab shows the connection history (Ops.CxLog.Events).
    internal sealed class ErrorLogForm : Form
    {
        private readonly DataGridView _grid = NewGrid();
        private readonly DataGridView _events = NewGrid();
        private readonly TabControl _tabs = new() { Dock = DockStyle.Fill };
        private readonly TabPage _cellsPage = new("Cells");
        private readonly TabPage _connectionPage = new("Connection");
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

            _events.Columns.Add("time", "Time (UTC)");
            _events.Columns.Add("event", "Event");
            _events.Columns[0].FillWeight = 16;
            _events.Columns[1].FillWeight = 84;

            _cellsPage.Controls.Add(_grid);
            _connectionPage.Controls.Add(_events);
            _tabs.TabPages.Add(_cellsPage);
            _tabs.TabPages.Add(_connectionPage);
            _tabs.SelectedIndexChanged += (_, _) => Reload();

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(8, 6, 8, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Refresh", (_, _) => Reload()),
                NewButton("Clear log", (_, _) => ClearSelected()),
                _status,
                NewButton("Close", (_, _) => Close()),
            });

            Controls.Add(_tabs);
            Controls.Add(bottom);

            Reload();
        }

        private static DataGridView NewGrid() => new()
        {
            Dock = DockStyle.Fill,
            ReadOnly = true,
            AllowUserToAddRows = false,
            AllowUserToResizeRows = false,
            RowHeadersVisible = false,
            SelectionMode = DataGridViewSelectionMode.FullRowSelect,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
        };

        private void ClearSelected()
        {
            if (_tabs.SelectedTab == _connectionPage) CxLog.Events.Clear();
            else CxErrorStore.Clear();
            Reload();
        }

        private void Reload()
        {
            _grid.Rows.Clear();
            _events.Rows.Clear();
            var entries = CxErrorStore.Snapshot();
            foreach (var d in entries)
                _grid.Rows.Add(d.Utc.ToString("HH:mm:ss"), d.Address, d.Code, d.Message);
            var events = CxLog.Events.Snapshot();
            foreach (var e in events)
                _events.Rows.Add(e.Utc.ToString("HH:mm:ss"), e.Text);
            _status.Text = _tabs.SelectedTab == _connectionPage
                ? $"{events.Count} connection event(s), newest first" + (CxLog.FilePath.Length > 0 ? " — log: " + CxLog.FilePath : "")
                : $"{entries.Count} error(s) recorded — double-click to jump to the cell";
        }

        private void GoTo(int rowIndex)
        {
            var address = (string?)_grid.Rows[rowIndex].Cells[1].Value;
            if (string.IsNullOrEmpty(address) || address == "(non-cell)" || !address!.Contains("!"))
                return;
            try
            {
                // Quote the sheet and escape embedded apostrophes so
                // "O'Brien!B2" resolves as 'O''Brien'!B2.
                int sep = address!.LastIndexOf('!');
                var sheet = address.Substring(0, sep).Replace("'", "''");
                var cell = address.Substring(sep + 1);
                ((dynamic)ExcelDnaUtil.Application).Goto($"'{sheet}'!{cell}", true);
            }
            catch
            {
                // Worker-thread captures store a stable key Goto can't parse.
            }
        }
    }
}
