using System;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using Convex.Excel.Rtd;
using ExcelDna.Integration.CustomUI;

namespace Convex.Excel
{
    /// <summary>
    /// Ribbon controller for the Convex Excel Add-In.
    /// Handles ribbon button callbacks and custom icon loading.
    /// </summary>
    [ComVisible(true)]
    public class RibbonController : ExcelRibbon
    {
        public override string GetCustomUI(string ribbonId)
        {
            return base.GetCustomUI(ribbonId);
        }

        /// <summary>
        /// Loads custom images for ribbon buttons.
        /// </summary>
        public override object LoadImage(string imageId)
        {
            const int size = 32;
            var bmp = new Bitmap(size, size);

            using (var g = Graphics.FromImage(bmp))
            {
                g.SmoothingMode = SmoothingMode.AntiAlias;
                g.Clear(Color.Transparent);

                switch (imageId)
                {
                    case "curve_new":
                        DrawCurveNewIcon(g, size);
                        break;
                    case "curve_view":
                        DrawCurveViewIcon(g, size);
                        break;
                    case "bond_new":
                        DrawBondNewIcon(g, size);
                        break;
                    case "bond_analyze":
                        DrawBondAnalyzeIcon(g, size);
                        break;
                    case "objects":
                        DrawObjectsIcon(g, size);
                        break;
                    case "clear":
                        DrawClearIcon(g, size);
                        break;
                    case "help":
                        DrawHelpIcon(g, size);
                        break;
                    case "about":
                        DrawAboutIcon(g, size);
                        break;
                    case "bootstrap":
                        DrawBootstrapIcon(g, size);
                        break;
                    case "rtd_settings":
                        DrawRtdSettingsIcon(g, size);
                        break;
                    case "rtd_toggle":
                        DrawRtdToggleIcon(g, size);
                        break;
                    case "rtd_refresh":
                        DrawRtdRefreshIcon(g, size);
                        break;
                    default:
                        DrawDefaultIcon(g, size);
                        break;
                }
            }

            return bmp;
        }

        private void DrawCurveNewIcon(Graphics g, int size)
        {
            // Blue curve with plus sign
            using (var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f))
            {
                var points = new PointF[]
                {
                    new PointF(4, 24),
                    new PointF(10, 20),
                    new PointF(16, 10),
                    new PointF(22, 8),
                    new PointF(28, 6)
                };
                g.DrawCurve(pen, points, 0.5f);
            }
            // Plus sign
            using (var pen = new Pen(Color.FromArgb(0, 180, 0), 2f))
            {
                g.DrawLine(pen, 22, 18, 22, 28);
                g.DrawLine(pen, 17, 23, 27, 23);
            }
        }

        private void DrawCurveViewIcon(Graphics g, int size)
        {
            // Blue curve
            using (var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f))
            {
                var points = new PointF[]
                {
                    new PointF(4, 26),
                    new PointF(10, 22),
                    new PointF(16, 12),
                    new PointF(22, 10),
                    new PointF(28, 8)
                };
                g.DrawCurve(pen, points, 0.5f);
            }
            // Magnifier
            using (var pen = new Pen(Color.FromArgb(80, 80, 80), 2f))
            {
                g.DrawEllipse(pen, 18, 16, 10, 10);
                g.DrawLine(pen, 26, 24, 30, 28);
            }
        }

        private void DrawBondNewIcon(Graphics g, int size)
        {
            // Certificate/bond shape
            using (var brush = new SolidBrush(Color.FromArgb(255, 193, 7)))
            {
                g.FillRectangle(brush, 4, 6, 20, 16);
            }
            using (var pen = new Pen(Color.FromArgb(180, 140, 0), 1.5f))
            {
                g.DrawRectangle(pen, 4, 6, 20, 16);
                g.DrawLine(pen, 7, 10, 21, 10);
                g.DrawLine(pen, 7, 14, 18, 14);
                g.DrawLine(pen, 7, 18, 15, 18);
            }
            // Plus sign
            using (var pen = new Pen(Color.FromArgb(0, 180, 0), 2f))
            {
                g.DrawLine(pen, 24, 18, 24, 28);
                g.DrawLine(pen, 19, 23, 29, 23);
            }
        }

        private void DrawBondAnalyzeIcon(Graphics g, int size)
        {
            // Bar chart
            using (var brush = new SolidBrush(Color.FromArgb(0, 120, 215)))
            {
                g.FillRectangle(brush, 4, 18, 6, 10);
                g.FillRectangle(brush, 12, 12, 6, 16);
                g.FillRectangle(brush, 20, 6, 6, 22);
            }
            // Trend line
            using (var pen = new Pen(Color.FromArgb(220, 60, 60), 2f))
            {
                g.DrawLine(pen, 4, 22, 28, 4);
            }
        }

        private void DrawObjectsIcon(Graphics g, int size)
        {
            // Multiple rectangles representing objects
            using (var brush1 = new SolidBrush(Color.FromArgb(0, 120, 215)))
            using (var brush2 = new SolidBrush(Color.FromArgb(255, 193, 7)))
            using (var brush3 = new SolidBrush(Color.FromArgb(76, 175, 80)))
            {
                g.FillRectangle(brush1, 4, 4, 12, 10);
                g.FillRectangle(brush2, 16, 8, 12, 10);
                g.FillRectangle(brush3, 8, 18, 12, 10);
            }
            using (var pen = new Pen(Color.FromArgb(60, 60, 60), 1f))
            {
                g.DrawRectangle(pen, 4, 4, 12, 10);
                g.DrawRectangle(pen, 16, 8, 12, 10);
                g.DrawRectangle(pen, 8, 18, 12, 10);
            }
        }

        private void DrawClearIcon(Graphics g, int size)
        {
            // Trash can
            using (var pen = new Pen(Color.FromArgb(200, 60, 60), 2f))
            {
                g.DrawRectangle(pen, 8, 10, 16, 18);
                g.DrawLine(pen, 4, 10, 28, 10);
                g.DrawLine(pen, 12, 6, 20, 6);
                g.DrawLine(pen, 12, 6, 12, 10);
                g.DrawLine(pen, 20, 6, 20, 10);
                // Lines inside
                g.DrawLine(pen, 12, 14, 12, 24);
                g.DrawLine(pen, 16, 14, 16, 24);
                g.DrawLine(pen, 20, 14, 20, 24);
            }
        }

        private void DrawHelpIcon(Graphics g, int size)
        {
            // Question mark in circle
            using (var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f))
            {
                g.DrawEllipse(pen, 4, 4, 24, 24);
            }
            using (var font = new Font("Arial", 16, FontStyle.Bold))
            using (var brush = new SolidBrush(Color.FromArgb(0, 120, 215)))
            {
                var sf = new StringFormat { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
                g.DrawString("?", font, brush, new RectangleF(0, 0, size, size), sf);
            }
        }

        private void DrawAboutIcon(Graphics g, int size)
        {
            // Info icon (i in circle)
            using (var pen = new Pen(Color.FromArgb(100, 100, 100), 2f))
            {
                g.DrawEllipse(pen, 6, 6, 20, 20);
            }
            using (var font = new Font("Arial", 14, FontStyle.Bold))
            using (var brush = new SolidBrush(Color.FromArgb(100, 100, 100)))
            {
                var sf = new StringFormat { Alignment = StringAlignment.Center, LineAlignment = StringAlignment.Center };
                g.DrawString("i", font, brush, new RectangleF(0, 2, size, size), sf);
            }
        }

        private void DrawDefaultIcon(Graphics g, int size)
        {
            using (var brush = new SolidBrush(Color.FromArgb(150, 150, 150)))
            {
                g.FillRectangle(brush, 4, 4, 24, 24);
            }
        }

        private void DrawBootstrapIcon(Graphics g, int size)
        {
            // Upward curve with construction/building theme
            using (var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f))
            {
                // Draw curve foundation
                var points = new PointF[]
                {
                    new PointF(4, 26),
                    new PointF(10, 20),
                    new PointF(16, 14),
                    new PointF(22, 10),
                    new PointF(28, 6)
                };
                g.DrawCurve(pen, points, 0.5f);
            }

            // Draw data points (circles at curve points)
            using (var brush = new SolidBrush(Color.FromArgb(76, 175, 80)))
            {
                g.FillEllipse(brush, 8, 18, 5, 5);   // Point 1
                g.FillEllipse(brush, 14, 12, 5, 5);  // Point 2
                g.FillEllipse(brush, 20, 8, 5, 5);   // Point 3
                g.FillEllipse(brush, 26, 4, 5, 5);   // Point 4
            }

            // Draw small gear/cog to indicate calibration
            using (var pen = new Pen(Color.FromArgb(255, 152, 0), 1.5f))
            {
                g.DrawEllipse(pen, 2, 20, 8, 8);
                // Gear teeth
                g.DrawLine(pen, 6, 20, 6, 18);
                g.DrawLine(pen, 6, 28, 6, 30);
                g.DrawLine(pen, 2, 24, 0, 24);
                g.DrawLine(pen, 10, 24, 12, 24);
            }
        }

        private void DrawRtdSettingsIcon(Graphics g, int size)
        {
            // Real-time streaming with gear
            // Signal waves
            using (var pen = new Pen(Color.FromArgb(0, 150, 136), 2f))
            {
                g.DrawArc(pen, 4, 8, 12, 12, -60, 120);
                g.DrawArc(pen, 8, 10, 8, 8, -60, 120);
                g.DrawArc(pen, 12, 12, 4, 4, -60, 120);
            }
            // Gear for settings
            using (var pen = new Pen(Color.FromArgb(100, 100, 100), 2f))
            {
                g.DrawEllipse(pen, 18, 16, 10, 10);
                // Gear teeth
                g.DrawLine(pen, 23, 16, 23, 14);
                g.DrawLine(pen, 23, 26, 23, 28);
                g.DrawLine(pen, 18, 21, 16, 21);
                g.DrawLine(pen, 28, 21, 30, 21);
            }
        }

        private void DrawRtdToggleIcon(Graphics g, int size)
        {
            // Toggle switch (draw as rounded rectangle using path)
            using (var brush = new SolidBrush(Color.FromArgb(76, 175, 80)))
            using (var path = CreateRoundedRectangle(4, 10, 24, 12, 6))
            {
                g.FillPath(brush, path);
            }
            using (var brush = new SolidBrush(Color.White))
            {
                g.FillEllipse(brush, 18, 11, 10, 10);
            }
        }

        private GraphicsPath CreateRoundedRectangle(int x, int y, int width, int height, int radius)
        {
            var path = new GraphicsPath();
            path.AddArc(x, y, radius * 2, radius * 2, 180, 90);
            path.AddArc(x + width - radius * 2, y, radius * 2, radius * 2, 270, 90);
            path.AddArc(x + width - radius * 2, y + height - radius * 2, radius * 2, radius * 2, 0, 90);
            path.AddArc(x, y + height - radius * 2, radius * 2, radius * 2, 90, 90);
            path.CloseFigure();
            return path;
        }

        private void DrawRtdRefreshIcon(Graphics g, int size)
        {
            // Circular arrow for refresh
            using (var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f))
            {
                g.DrawArc(pen, 6, 6, 20, 20, 0, 300);
                // Arrow head
                g.DrawLine(pen, 24, 12, 26, 6);
                g.DrawLine(pen, 24, 12, 18, 8);
            }
        }

        // ========================================================================
        // Curves Group
        // ========================================================================

        public void OnNewCurve(IRibbonControl control)
        {
            using (var form = new NewCurveForm())
            {
                form.ShowDialog();
            }
        }

        public void OnCurveViewer(IRibbonControl control)
        {
            try
            {
                using (var form = new CurveViewerForm())
                {
                    form.ShowDialog();
                }
            }
            catch (Exception ex)
            {
                MessageBox.Show(
                    $"Error opening Curve Viewer:\n\n{ex.Message}\n\n{ex.StackTrace}",
                    "Error",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            }
        }

        public void OnBootstrap(IRibbonControl control)
        {
            try
            {
                using (var form = new BootstrapForm())
                {
                    form.ShowDialog();
                }
            }
            catch (Exception ex)
            {
                MessageBox.Show(
                    $"Error opening Bootstrap Form:\n\n{ex.Message}\n\n{ex.StackTrace}",
                    "Error",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Error);
            }
        }

        // ========================================================================
        // Bonds Group
        // ========================================================================

        public void OnNewBond(IRibbonControl control)
        {
            using (var form = new NewBondForm())
            {
                form.ShowDialog();
            }
        }

        public void OnBondAnalyzer(IRibbonControl control)
        {
            using (var form = new BondAnalyzerForm())
            {
                form.ShowDialog();
            }
        }

        // ========================================================================
        // Tools Group
        // ========================================================================

        public void OnObjectBrowser(IRibbonControl control)
        {
            using (var form = new ObjectBrowserForm())
            {
                form.ShowDialog();
            }
        }

        public void OnClearAll(IRibbonControl control)
        {
            var result = MessageBox.Show(
                "Are you sure you want to clear all registered objects?\n\n" +
                "This will invalidate all existing handles.",
                "Confirm Clear All",
                MessageBoxButtons.YesNo,
                MessageBoxIcon.Warning);

            if (result == DialogResult.Yes)
            {
                ConvexWrapper.ClearAll();
                MessageBox.Show(
                    "All objects have been cleared.",
                    "Clear Complete",
                    MessageBoxButtons.OK,
                    MessageBoxIcon.Information);
            }
        }

        // ========================================================================
        // RTD Group
        // ========================================================================

        public void OnRtdSettings(IRibbonControl control)
        {
            using (var form = new RtdSettingsForm())
            {
                form.ShowDialog();
            }
        }

        public void OnRtdToggle(IRibbonControl control, bool pressed)
        {
            RtdSettings.Enabled = pressed;
        }

        public bool GetRtdEnabled(IRibbonControl control)
        {
            return RtdSettings.Enabled;
        }

        public void OnRtdRefresh(IRibbonControl control)
        {
            RtdSettings.RefreshAll();
            MessageBox.Show(
                "All RTD topics have been refreshed.",
                "RTD Refresh",
                MessageBoxButtons.OK,
                MessageBoxIcon.Information);
        }

        public void OnHelp(IRibbonControl control)
        {
            using (var form = new HelpForm())
            {
                form.ShowDialog();
            }
        }

        public void OnAbout(IRibbonControl control)
        {
            string version = ConvexWrapper.GetVersion();

            MessageBox.Show(
                "Convex Excel Add-In\n\n" +
                "Version: " + version + "\n\n" +
                "High-performance fixed income analytics library.\n\n" +
                "Features:\n" +
                "  - Yield curve construction and queries\n" +
                "  - Bond pricing (YTM, price, accrued)\n" +
                "  - Risk metrics (duration, convexity, DV01)\n" +
                "  - Spread calculations (Z, I, G, ASW)\n" +
                "  - Callable bond support\n\n" +
                "Built with Rust + Excel-DNA",
                "About Convex",
                MessageBoxButtons.OK,
                MessageBoxIcon.Information);
        }
    }
}
