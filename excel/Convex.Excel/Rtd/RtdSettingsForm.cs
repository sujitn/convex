using System;
using System.Drawing;
using System.Windows.Forms;

namespace Convex.Excel.Rtd
{
    /// <summary>
    /// Settings form for RTD configuration.
    /// </summary>
    public class RtdSettingsForm : Form
    {
        private GroupBox grpStatus;
        private Label lblServerStatus;
        private Label lblTopicCount;
        private Button btnRefresh;

        private GroupBox grpControls;
        private CheckBox chkEnabled;
        private Button btnPause;
        private Button btnResume;

        private GroupBox grpIntervals;
        private Label lblUpdateInterval;
        private TrackBar trkUpdateInterval;
        private Label lblUpdateValue;
        private Label lblThrottle;
        private TrackBar trkThrottle;
        private Label lblThrottleValue;

        private GroupBox grpPresets;
        private RadioButton radHighFreq;
        private RadioButton radNormal;
        private RadioButton radLowFreq;
        private RadioButton radBatterySaver;

        private Button btnApply;
        private Button btnReset;
        private Button btnClose;

        private Timer _refreshTimer;

        public RtdSettingsForm()
        {
            InitializeComponents();
            LoadCurrentSettings();
            StartRefreshTimer();
        }

        private void InitializeComponents()
        {
            Text = "RTD Settings";
            Size = new Size(420, 520);
            StartPosition = FormStartPosition.CenterParent;
            FormBorderStyle = FormBorderStyle.FixedDialog;
            MaximizeBox = false;
            MinimizeBox = false;

            // Status Group
            grpStatus = new GroupBox
            {
                Text = "RTD Server Status",
                Location = new Point(12, 12),
                Size = new Size(380, 80)
            };

            lblServerStatus = new Label
            {
                Text = "Server: Checking...",
                Location = new Point(15, 25),
                Size = new Size(200, 20),
                Font = new Font(Font, FontStyle.Bold)
            };

            lblTopicCount = new Label
            {
                Text = "Active Topics: 0",
                Location = new Point(15, 50),
                Size = new Size(250, 20)
            };

            btnRefresh = new Button
            {
                Text = "Refresh All",
                Location = new Point(280, 35),
                Size = new Size(85, 28)
            };
            btnRefresh.Click += BtnRefresh_Click;

            grpStatus.Controls.AddRange(new Control[] { lblServerStatus, lblTopicCount, btnRefresh });

            // Controls Group
            grpControls = new GroupBox
            {
                Text = "RTD Controls",
                Location = new Point(12, 100),
                Size = new Size(380, 70)
            };

            chkEnabled = new CheckBox
            {
                Text = "RTD Updates Enabled",
                Location = new Point(15, 28),
                Size = new Size(150, 25),
                Checked = RtdSettings.Enabled
            };
            chkEnabled.CheckedChanged += ChkEnabled_CheckedChanged;

            btnPause = new Button
            {
                Text = "Pause",
                Location = new Point(200, 25),
                Size = new Size(75, 28)
            };
            btnPause.Click += BtnPause_Click;

            btnResume = new Button
            {
                Text = "Resume",
                Location = new Point(285, 25),
                Size = new Size(75, 28)
            };
            btnResume.Click += BtnResume_Click;

            grpControls.Controls.AddRange(new Control[] { chkEnabled, btnPause, btnResume });

            // Intervals Group
            grpIntervals = new GroupBox
            {
                Text = "Update Intervals",
                Location = new Point(12, 178),
                Size = new Size(380, 120)
            };

            lblUpdateInterval = new Label
            {
                Text = "Update Interval:",
                Location = new Point(15, 28),
                Size = new Size(100, 20)
            };

            trkUpdateInterval = new TrackBar
            {
                Location = new Point(120, 22),
                Size = new Size(200, 45),
                Minimum = 50,
                Maximum = 2000,
                TickFrequency = 100,
                Value = Math.Min(2000, Math.Max(50, RtdSettings.UpdateIntervalMs))
            };
            trkUpdateInterval.ValueChanged += TrkUpdateInterval_ValueChanged;

            lblUpdateValue = new Label
            {
                Text = $"{RtdSettings.UpdateIntervalMs} ms",
                Location = new Point(325, 28),
                Size = new Size(50, 20)
            };

            lblThrottle = new Label
            {
                Text = "Throttle:",
                Location = new Point(15, 75),
                Size = new Size(100, 20)
            };

            trkThrottle = new TrackBar
            {
                Location = new Point(120, 69),
                Size = new Size(200, 45),
                Minimum = 100,
                Maximum = 5000,
                TickFrequency = 250,
                Value = Math.Min(5000, Math.Max(100, RtdSettings.ThrottleMs))
            };
            trkThrottle.ValueChanged += TrkThrottle_ValueChanged;

            lblThrottleValue = new Label
            {
                Text = $"{RtdSettings.ThrottleMs} ms",
                Location = new Point(325, 75),
                Size = new Size(50, 20)
            };

            grpIntervals.Controls.AddRange(new Control[] {
                lblUpdateInterval, trkUpdateInterval, lblUpdateValue,
                lblThrottle, trkThrottle, lblThrottleValue
            });

            // Presets Group
            grpPresets = new GroupBox
            {
                Text = "Presets",
                Location = new Point(12, 306),
                Size = new Size(380, 100)
            };

            radHighFreq = new RadioButton
            {
                Text = "High Frequency (50ms)",
                Location = new Point(15, 25),
                Size = new Size(160, 25)
            };
            radHighFreq.CheckedChanged += RadPreset_CheckedChanged;

            radNormal = new RadioButton
            {
                Text = "Normal (200ms)",
                Location = new Point(200, 25),
                Size = new Size(140, 25),
                Checked = true
            };
            radNormal.CheckedChanged += RadPreset_CheckedChanged;

            radLowFreq = new RadioButton
            {
                Text = "Low Frequency (1000ms)",
                Location = new Point(15, 60),
                Size = new Size(170, 25)
            };
            radLowFreq.CheckedChanged += RadPreset_CheckedChanged;

            radBatterySaver = new RadioButton
            {
                Text = "Battery Saver (2000ms)",
                Location = new Point(200, 60),
                Size = new Size(170, 25)
            };
            radBatterySaver.CheckedChanged += RadPreset_CheckedChanged;

            grpPresets.Controls.AddRange(new Control[] { radHighFreq, radNormal, radLowFreq, radBatterySaver });

            // Bottom buttons
            btnApply = new Button
            {
                Text = "Apply",
                Location = new Point(135, 420),
                Size = new Size(80, 30)
            };
            btnApply.Click += BtnApply_Click;

            btnReset = new Button
            {
                Text = "Reset",
                Location = new Point(225, 420),
                Size = new Size(80, 30)
            };
            btnReset.Click += BtnReset_Click;

            btnClose = new Button
            {
                Text = "Close",
                Location = new Point(315, 420),
                Size = new Size(80, 30),
                DialogResult = DialogResult.Cancel
            };

            Controls.AddRange(new Control[] {
                grpStatus, grpControls, grpIntervals, grpPresets,
                btnApply, btnReset, btnClose
            });

            CancelButton = btnClose;
        }

        private void LoadCurrentSettings()
        {
            chkEnabled.Checked = RtdSettings.Enabled;
            trkUpdateInterval.Value = Math.Min(trkUpdateInterval.Maximum, Math.Max(trkUpdateInterval.Minimum, RtdSettings.UpdateIntervalMs));
            trkThrottle.Value = Math.Min(trkThrottle.Maximum, Math.Max(trkThrottle.Minimum, RtdSettings.ThrottleMs));
            UpdateStatusDisplay();
        }

        private void StartRefreshTimer()
        {
            _refreshTimer = new Timer { Interval = 1000 };
            _refreshTimer.Tick += (s, e) => UpdateStatusDisplay();
            _refreshTimer.Start();
        }

        private void UpdateStatusDisplay()
        {
            if (InvokeRequired)
            {
                Invoke(new Action(UpdateStatusDisplay));
                return;
            }

            bool running = RtdSettings.IsServerRunning;
            lblServerStatus.Text = running ? "Server: Running" : "Server: Not Started";
            lblServerStatus.ForeColor = running ? Color.Green : Color.Gray;

            var stats = RtdSettings.GetStatistics();
            lblTopicCount.Text = $"Active Topics: {stats.total} (Curves: {stats.curves}, Bonds: {stats.bonds}, Analytics: {stats.analytics})";

            btnPause.Enabled = RtdSettings.Enabled;
            btnResume.Enabled = !RtdSettings.Enabled;
        }

        private void ChkEnabled_CheckedChanged(object sender, EventArgs e)
        {
            RtdSettings.Enabled = chkEnabled.Checked;
            UpdateStatusDisplay();
        }

        private void BtnPause_Click(object sender, EventArgs e)
        {
            RtdSettings.Pause();
            chkEnabled.Checked = false;
            UpdateStatusDisplay();
        }

        private void BtnResume_Click(object sender, EventArgs e)
        {
            RtdSettings.Resume();
            chkEnabled.Checked = true;
            UpdateStatusDisplay();
        }

        private void BtnRefresh_Click(object sender, EventArgs e)
        {
            RtdSettings.RefreshAll();
            MessageBox.Show("Refresh triggered for all topics.", "Refresh", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        private void TrkUpdateInterval_ValueChanged(object sender, EventArgs e)
        {
            lblUpdateValue.Text = $"{trkUpdateInterval.Value} ms";
        }

        private void TrkThrottle_ValueChanged(object sender, EventArgs e)
        {
            lblThrottleValue.Text = $"{trkThrottle.Value} ms";
        }

        private void RadPreset_CheckedChanged(object sender, EventArgs e)
        {
            if (!(sender is RadioButton rad) || !rad.Checked) return;

            if (rad == radHighFreq)
            {
                trkUpdateInterval.Value = 50;
                trkThrottle.Value = 100;
            }
            else if (rad == radNormal)
            {
                trkUpdateInterval.Value = 200;
                trkThrottle.Value = 500;
            }
            else if (rad == radLowFreq)
            {
                trkUpdateInterval.Value = 1000;
                trkThrottle.Value = 2000;
            }
            else if (rad == radBatterySaver)
            {
                trkUpdateInterval.Value = 2000;
                trkThrottle.Value = 5000;
            }
        }

        private void BtnApply_Click(object sender, EventArgs e)
        {
            RtdSettings.UpdateIntervalMs = trkUpdateInterval.Value;
            RtdSettings.ThrottleMs = trkThrottle.Value;
            MessageBox.Show("Settings applied.", "Settings", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        private void BtnReset_Click(object sender, EventArgs e)
        {
            RtdSettings.ResetToDefaults();
            LoadCurrentSettings();
            radNormal.Checked = true;
            MessageBox.Show("Settings reset to defaults.", "Settings", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        protected override void OnFormClosing(FormClosingEventArgs e)
        {
            _refreshTimer?.Stop();
            _refreshTimer?.Dispose();
            base.OnFormClosing(e);
        }
    }
}
