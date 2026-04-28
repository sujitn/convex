using System;
using System.Drawing;
using System.Windows.Forms;

namespace Convex.Excel.Forms
{
    // Edit and persist the small settings struct. UDFs and ribbon forms read
    // CxSettings.Current and use these values when an argument is omitted.
    internal sealed class SettingsForm : Form
    {
        private readonly ComboBox _frequency = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly ComboBox _dayCount = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly ComboBox _spreadType = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly ComboBox _currency = new() { DropDownStyle = ComboBoxStyle.DropDownList };
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public SettingsForm()
        {
            Text = "Convex — Settings";
            Size = new Size(440, 280);
            StartPosition = FormStartPosition.CenterParent;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = MinimizeBox = false;

            _frequency.Items.AddRange(new object[] { "Annual", "SemiAnnual", "Quarterly", "Monthly" });
            _dayCount.Items.AddRange(new object[] { "Act360", "Act365Fixed", "ActActIsda", "ActActIcma", "Thirty360US", "Thirty360E" });
            _spreadType.Items.AddRange(new object[] { "Z", "G", "I", "OAS", "DM", "ASW_PAR", "ASW_PROC" });
            _currency.Items.AddRange(new object[] { "USD", "EUR", "GBP", "JPY", "CHF", "CAD", "AUD" });

            var s = CxSettings.Current;
            _frequency.SelectedItem = s.DefaultFrequency;
            _dayCount.SelectedItem = s.DefaultDayCount;
            _spreadType.SelectedItem = s.DefaultSpreadType;
            _currency.SelectedItem = s.DefaultCurrency;

            var grid = new TableLayoutPanel
            {
                Dock = DockStyle.Top, Height = 180,
                ColumnCount = 2, RowCount = 4, Padding = new Padding(12),
            };
            grid.ColumnStyles.Add(new ColumnStyle(SizeType.Absolute, 160));
            grid.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 4; i++) grid.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            AddRow(grid, 0, "Default frequency:", _frequency);
            AddRow(grid, 1, "Default day count:", _dayCount);
            AddRow(grid, 2, "Default spread:", _spreadType);
            AddRow(grid, 3, "Default currency:", _currency);

            var bottom = new FlowLayoutPanel
            {
                Dock = DockStyle.Bottom, Height = 44, Padding = new Padding(12, 6, 12, 6),
            };
            bottom.Controls.AddRange(new Control[]
            {
                NewButton("Save", (_,_) => Save()),
                NewButton("Reset", (_,_) => Reset()),
                _status,
                NewButton("Close", (_,_) => Close()),
            });

            Controls.Add(grid);
            Controls.Add(bottom);
        }

        private void Save()
        {
            try
            {
                CxSettings.Save(new CxSettings.Snapshot
                {
                    DefaultFrequency = (string)_frequency.SelectedItem!,
                    DefaultDayCount = (string)_dayCount.SelectedItem!,
                    DefaultSpreadType = (string)_spreadType.SelectedItem!,
                    DefaultCurrency = (string)_currency.SelectedItem!,
                });
                _status.Text = "Saved";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        private void Reset()
        {
            var d = new CxSettings.Snapshot();
            _frequency.SelectedItem = d.DefaultFrequency;
            _dayCount.SelectedItem = d.DefaultDayCount;
            _spreadType.SelectedItem = d.DefaultSpreadType;
            _currency.SelectedItem = d.DefaultCurrency;
            _status.Text = "Reset (not saved)";
        }

        private static void AddRow(TableLayoutPanel grid, int row, string label, Control control)
        {
            grid.Controls.Add(
                new Label { Text = label, Anchor = AnchorStyles.Left, AutoSize = true, Padding = new Padding(0, 6, 0, 0) },
                0, row);
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
