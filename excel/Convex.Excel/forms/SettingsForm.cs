using System;
using System.Drawing;
using System.Windows.Forms;
using static Convex.Excel.Helpers.FormUi;

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
        private readonly CheckBox _autoRebuild = new()
        {
            Text = "Rebuild handles when the add-in loads",
            AutoSize = true,
        };
        // Excel's RTD throttle, not a Convex setting: shown, and changed only
        // on request (DX8), because it applies to every add-in.
        private readonly Label _throttle = new() { AutoSize = true, Padding = new Padding(0, 6, 8, 0) };
        private readonly Button _fastThrottle = new() { Text = "Use 250 ms", AutoSize = true };
        private readonly Button _defaultThrottle = new() { Text = "Excel default", AutoSize = true };
        private readonly CheckBox _checkUpdates = new()
        {
            Text = "Check once a day for a newer release",
            AutoSize = true,
        };
        private readonly TextBox _updateFeed = new();
        // What policy or the installer decided; Save keeps the saved values they hide.
        private readonly Ops.UpdateConfig _update = Ops.UpdateConfig.Read();
        private readonly ToolTip _tips = new();
        private readonly Label _status = new() { AutoSize = true, ForeColor = Color.Gray };

        public SettingsForm()
        {
            Text = "Convex — Settings";
            Size = new Size(520, 360);
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
            _autoRebuild.Checked = s.AutoRebuildOnOpen;
            _fastThrottle.Click += (_, _) => SetThrottle(250);
            _defaultThrottle.Click += (_, _) => SetThrottle(2000);
            _tips.SetToolTip(_fastThrottle,
                "Live cells update at most once per Excel RTD throttle interval (default 2,000 ms).\n" +
                "This setting belongs to Excel: it applies to every add-in's live cells and\n" +
                "persists after Excel restarts. Very low values can make Excel sluggish.");
            ShowThrottle();
            ShowUpdate(s);

            var grid = new TableLayoutPanel
            {
                Dock = DockStyle.Top, Height = 324,
                ColumnCount = 2, RowCount = 8, Padding = new Padding(12),
            };
            grid.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            grid.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            for (int i = 0; i < 8; i++) grid.RowStyles.Add(new RowStyle(SizeType.Absolute, 36));
            AddRow(grid, 0, "Default frequency:", _frequency);
            AddRow(grid, 1, "Default day count:", _dayCount);
            AddRow(grid, 2, "Default spread:", _spreadType);
            AddRow(grid, 3, "Default currency:", _currency);
            AddRow(grid, 4, "On load:", _autoRebuild);
            var throttleRow = new FlowLayoutPanel { AutoSize = true, WrapContents = false };
            throttleRow.Controls.AddRange(new Control[] { _throttle, _fastThrottle, _defaultThrottle });
            AddRow(grid, 5, "Excel RTD throttle:", throttleRow);
            AddRow(grid, 6, "Updates:", _checkUpdates);
            AddRow(grid, 7, "Update feed:", _updateFeed);

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
                    AutoRebuildOnOpen = _autoRebuild.Checked,
                    CheckForUpdates = _checkUpdates.Enabled ? _checkUpdates.Checked : CxSettings.Current.CheckForUpdates,
                    UpdateFeed = _updateFeed.Enabled ? _updateFeed.Text.Trim() : CxSettings.Current.UpdateFeed,
                });
                Ops.UpdateCheck.SettingsChanged();
                _status.Text = "Saved";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
        }

        // Policy, then the installer, can turn the check off; policy can also set the feed.
        private void ShowUpdate(CxSettings.Snapshot s)
        {
            var locked = _update.Switch switch
            {
                Ops.UpdateSwitch.OffByPolicy => " (turned off by policy)",
                Ops.UpdateSwitch.OffByInstaller => " (turned off by the installer)",
                _ => null,
            };
            _checkUpdates.Checked = locked == null && s.CheckForUpdates;
            _checkUpdates.Enabled = locked == null;
            if (locked != null) _checkUpdates.Text += locked;
            _updateFeed.Text = _update.FeedByPolicy ? _update.Feed : s.UpdateFeed;
            _updateFeed.Enabled = !_update.FeedByPolicy;
            _tips.SetToolTip(_checkUpdates,
                "Reads where the feed redirects to, without following it, and shows Update on the\n" +
                "Help group when a newer version is out. Nothing is downloaded or installed.");
            _tips.SetToolTip(_updateFeed,
                "Blank: GitHub's latest release. A mirror's address must redirect to the latest\n" +
                "release's page, whose last path segment is its version, such as .../tag/0.15.0.");
        }

        private void Reset()
        {
            var d = new CxSettings.Snapshot();
            _frequency.SelectedItem = d.DefaultFrequency;
            _dayCount.SelectedItem = d.DefaultDayCount;
            _spreadType.SelectedItem = d.DefaultSpreadType;
            _currency.SelectedItem = d.DefaultCurrency;
            _autoRebuild.Checked = d.AutoRebuildOnOpen;
            if (_checkUpdates.Enabled) _checkUpdates.Checked = d.CheckForUpdates;
            if (_updateFeed.Enabled) _updateFeed.Text = d.UpdateFeed;
            _status.Text = "Reset (not saved)";
        }

        private void ShowThrottle()
        {
            try
            {
                int ms = (int)((dynamic)ExcelDna.Integration.ExcelDnaUtil.Application).RTD.ThrottleInterval;
                _throttle.Text = $"{ms:N0} ms";
                _fastThrottle.Enabled = ms != 250;
                _defaultThrottle.Enabled = ms != 2000;
            }
            catch (Exception ex)
            {
                _throttle.Text = "unavailable: " + ex.Message;
                _fastThrottle.Enabled = _defaultThrottle.Enabled = false;
            }
        }

        private void SetThrottle(int ms)
        {
            try
            {
                ((dynamic)ExcelDna.Integration.ExcelDnaUtil.Application).RTD.ThrottleInterval = ms;
                _status.Text = $"Excel RTD throttle set to {ms:N0} ms";
            }
            catch (Exception ex) { _status.Text = "ERROR: " + ex.Message; }
            ShowThrottle();
        }

        private static void AddRow(TableLayoutPanel grid, int row, string label, Control control)
        {
            grid.Controls.Add(
                new Label { Text = label, Anchor = AnchorStyles.Left, AutoSize = true, Padding = new Padding(0, 6, 0, 0) },
                0, row);
            control.Anchor = AnchorStyles.Left | AnchorStyles.Right;
            grid.Controls.Add(control, 1, row);
        }
    }
}
