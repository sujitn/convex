using System;
using System.Drawing;
using System.Linq;
using System.Windows.Forms;
using Convex.Excel.Helpers;

namespace Convex.Excel.Forms
{
    // Lists every registered object via convex_list_objects. Lets the user:
    //  • paste a handle (or its name) into the active worksheet cell;
    //  • describe the object (calls convex_describe);
    //  • release one or many handles;
    //  • filter by kind (curve / fixed_rate / callable / floating_rate / ...).
    internal sealed class ObjectBrowserForm : Form
    {
        private readonly DataGridView _grid = new()
        {
            Dock = DockStyle.Fill,
            ReadOnly = true,
            AllowUserToAddRows = false,
            AllowUserToDeleteRows = false,
            AllowUserToResizeRows = false,
            AutoSizeColumnsMode = DataGridViewAutoSizeColumnsMode.Fill,
            SelectionMode = DataGridViewSelectionMode.FullRowSelect,
            MultiSelect = true,
            RowHeadersVisible = false,
        };
        private readonly ComboBox _filter = new() { DropDownStyle = ComboBoxStyle.DropDownList, Width = 180 };
        private readonly TextBox _search = new() { Width = 220 };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public ObjectBrowserForm()
        {
            Text = "Convex — Object Browser";
            Size = new Size(720, 480);
            MinimumSize = new Size(540, 320);
            StartPosition = FormStartPosition.CenterParent;

            _grid.Columns.Add("Handle", "Handle");
            _grid.Columns.Add("Kind", "Kind");
            _grid.Columns.Add("Name", "Name");
            _grid.Columns[0].DefaultCellStyle.Font = new Font("Consolas", 9f);

            var top = new FlowLayoutPanel
            {
                Dock = DockStyle.Top,
                Height = 38,
                Padding = new Padding(8, 6, 8, 6),
            };
            _filter.Items.Add("All");
            _filter.Items.AddRange(new object[] { "curve", "fixed_rate", "callable", "floating_rate", "zero_coupon", "sinking_fund" });
            _filter.SelectedIndex = 0;
            _filter.SelectedIndexChanged += (_, _) => Refresh1();
            _search.TextChanged += (_, _) => Refresh1();
            top.Controls.Add(new Label { Text = "Kind:", AutoSize = true, Padding = new Padding(0, 4, 4, 0) });
            top.Controls.Add(_filter);
            top.Controls.Add(new Label { Text = "Name filter:", AutoSize = true, Padding = new Padding(8, 4, 4, 0) });
            top.Controls.Add(_search);

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom,
                Height = 44,
                Padding = new Padding(8, 6, 8, 6),
                FlowDirection = FlowDirection.LeftToRight,
            };
            var btnPaste = NewButton("Paste handle into cell", PasteHandle);
            var btnDescribe = NewButton("Describe", Describe);
            var btnRelease = NewButton("Release", Release);
            var btnRefresh = NewButton("Refresh", (_, _) => Refresh1());
            var btnClose = NewButton("Close", (_, _) => Close());
            bottom.Controls.AddRange(new Control[] { btnPaste, btnDescribe, btnRelease, btnRefresh, _status, btnClose });

            Controls.Add(_grid);
            Controls.Add(top);
            Controls.Add(bottom);

            Refresh1();
        }

        private void Refresh1()
        {
            try
            {
                var entries = Cx.ListObjects();
                string kind = (string)(_filter.SelectedItem ?? "All");
                string needle = _search.Text.Trim();
                var filtered = entries
                    .Where(e => kind == "All" || string.Equals(e.Kind, kind, StringComparison.OrdinalIgnoreCase))
                    .Where(e => needle.Length == 0 || (e.Name ?? "").IndexOf(needle, StringComparison.OrdinalIgnoreCase) >= 0)
                    .OrderBy(e => e.Handle)
                    .ToList();
                _grid.Rows.Clear();
                foreach (var e in filtered)
                    _grid.Rows.Add(CxParse.FormatHandle(e.Handle), e.Kind, e.Name ?? "");
                _status.Text = $"{filtered.Count} of {entries.Count} object(s)";
            }
            catch (Exception ex)
            {
                _status.Text = "ERROR: " + ex.Message;
            }
        }

        private void PasteHandle(object? _, EventArgs __)
        {
            if (_grid.SelectedRows.Count == 0) return;
            var handleText = (string)_grid.SelectedRows[0].Cells[0].Value;
            try
            {
                var addr = SheetHelpers.WriteFormulaAtSelection("=\"" + handleText + "\"");
                SheetHelpers.Status($"Pasted {handleText} at {addr}");
            }
            catch (Exception ex)
            {
                MessageBox.Show(ex.Message, "Paste failed", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        private void Describe(object? _, EventArgs __)
        {
            if (_grid.SelectedRows.Count == 0) return;
            try
            {
                var handleText = (string)_grid.SelectedRows[0].Cells[0].Value;
                var handle = CxParse.AsHandle(handleText, "handle");
                var desc = Cx.Describe(handle);
                MessageBox.Show(desc, "Describe " + handleText, MessageBoxButtons.OK, MessageBoxIcon.Information);
            }
            catch (Exception ex)
            {
                MessageBox.Show(ex.Message, "Describe failed", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private void Release(object? _, EventArgs __)
        {
            if (_grid.SelectedRows.Count == 0) return;
            var rows = _grid.SelectedRows.Cast<DataGridViewRow>().ToList();
            var ok = MessageBox.Show($"Release {rows.Count} object(s)?",
                "Confirm release", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
            if (ok != DialogResult.Yes) return;
            int released = 0;
            foreach (var row in rows)
            {
                try
                {
                    var handle = CxParse.AsHandle((string)row.Cells[0].Value, "handle");
                    Cx.Release(handle);
                    released++;
                }
                catch { /* keep going */ }
            }
            Refresh1();
            SheetHelpers.Status($"Released {released} of {rows.Count}");
        }

        private static Button NewButton(string text, EventHandler onClick)
        {
            var b = new Button { Text = text, AutoSize = true, Padding = new Padding(8, 2, 8, 2) };
            b.Click += onClick;
            return b;
        }
    }
}
