using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Drawing2D;

namespace Convex.Excel.Helpers
{
    // A tiny atlas of programmatic 32×32 ribbon icons. The right long-term
    // answer is a Resources/icons/*.png set; until those exist, this keeps
    // the paint code in one place so the rest of the codebase doesn't have
    // to know that ribbon icons are GDI+ scribbles.
    //
    // Bitmaps are cached on first paint and reused for the lifetime of the
    // process. Add a new icon: register one (name, painter) pair below.
    internal static class IconAtlas
    {
        private const int Size = 32;

        private static readonly Dictionary<string, Action<Graphics>> Painters = new()
        {
            ["bond_new"] = BondNew,
            ["bond_analyze"] = BondAnalyze,
            ["curve_new"] = CurveNew,
            ["curve_view"] = CurveView,
            ["bootstrap"] = Bootstrap,
            ["objects"] = Objects,
            ["clear"] = Clear,
            ["about"] = About,
        };

        private static readonly Dictionary<string, Image> Cache = new();

        public static Image Get(string id)
        {
            if (Cache.TryGetValue(id, out var cached)) return cached;
            var bmp = new Bitmap(Size, Size);
            using (var g = Graphics.FromImage(bmp))
            {
                g.SmoothingMode = SmoothingMode.AntiAlias;
                g.Clear(Color.Transparent);
                if (Painters.TryGetValue(id, out var painter)) painter(g);
                else Default(g);
            }
            Cache[id] = bmp;
            return bmp;
        }

        // ---- painters --------------------------------------------------

        private static void BondNew(Graphics g)
        {
            using var fill = new SolidBrush(Color.FromArgb(255, 193, 7));
            using var border = new Pen(Color.FromArgb(180, 140, 0), 1.5f);
            using var plus = new Pen(Color.FromArgb(0, 180, 0), 2f);
            g.FillRectangle(fill, 4, 6, 20, 16);
            g.DrawRectangle(border, 4, 6, 20, 16);
            g.DrawLine(border, 7, 10, 21, 10);
            g.DrawLine(border, 7, 14, 18, 14);
            g.DrawLine(border, 7, 18, 15, 18);
            g.DrawLine(plus, 24, 18, 24, 28);
            g.DrawLine(plus, 19, 23, 29, 23);
        }

        private static void BondAnalyze(Graphics g)
        {
            using var bars = new SolidBrush(Color.FromArgb(0, 120, 215));
            using var trend = new Pen(Color.FromArgb(220, 60, 60), 2f);
            g.FillRectangle(bars, 4, 18, 6, 10);
            g.FillRectangle(bars, 12, 12, 6, 16);
            g.FillRectangle(bars, 20, 6, 6, 22);
            g.DrawLine(trend, 4, 22, 28, 4);
        }

        private static void CurveNew(Graphics g)
        {
            using var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f);
            using var plus = new Pen(Color.FromArgb(0, 180, 0), 2f);
            g.DrawCurve(pen, CurvePath(24));
            g.DrawLine(plus, 22, 18, 22, 28);
            g.DrawLine(plus, 17, 23, 27, 23);
        }

        private static void CurveView(Graphics g)
        {
            using var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f);
            using var glass = new Pen(Color.FromArgb(80, 80, 80), 2f);
            g.DrawCurve(pen, CurvePath(26));
            g.DrawEllipse(glass, 18, 16, 10, 10);
            g.DrawLine(glass, 26, 24, 30, 28);
        }

        private static void Bootstrap(Graphics g)
        {
            using var pen = new Pen(Color.FromArgb(0, 120, 215), 2.5f);
            using var dots = new SolidBrush(Color.FromArgb(76, 175, 80));
            g.DrawCurve(pen, CurvePath(26));
            g.FillEllipse(dots, 8, 18, 5, 5);
            g.FillEllipse(dots, 14, 12, 5, 5);
            g.FillEllipse(dots, 20, 8, 5, 5);
            g.FillEllipse(dots, 26, 4, 5, 5);
        }

        private static void Objects(Graphics g)
        {
            using var b1 = new SolidBrush(Color.FromArgb(0, 120, 215));
            using var b2 = new SolidBrush(Color.FromArgb(255, 193, 7));
            using var b3 = new SolidBrush(Color.FromArgb(76, 175, 80));
            using var pen = new Pen(Color.FromArgb(60, 60, 60), 1f);
            g.FillRectangle(b1, 4, 4, 12, 10);
            g.FillRectangle(b2, 16, 8, 12, 10);
            g.FillRectangle(b3, 8, 18, 12, 10);
            g.DrawRectangle(pen, 4, 4, 12, 10);
            g.DrawRectangle(pen, 16, 8, 12, 10);
            g.DrawRectangle(pen, 8, 18, 12, 10);
        }

        private static void Clear(Graphics g)
        {
            using var pen = new Pen(Color.FromArgb(200, 60, 60), 2f);
            g.DrawRectangle(pen, 8, 10, 16, 18);
            g.DrawLine(pen, 4, 10, 28, 10);
            g.DrawLine(pen, 12, 6, 20, 6);
            g.DrawLine(pen, 12, 6, 12, 10);
            g.DrawLine(pen, 20, 6, 20, 10);
            for (int x = 12; x <= 20; x += 4) g.DrawLine(pen, x, 14, x, 24);
        }

        private static void About(Graphics g)
        {
            using var pen = new Pen(Color.FromArgb(100, 100, 100), 2f);
            using var brush = new SolidBrush(Color.FromArgb(100, 100, 100));
            using var font = new Font("Arial", 14, FontStyle.Bold);
            var sf = new StringFormat
            {
                Alignment = StringAlignment.Center,
                LineAlignment = StringAlignment.Center,
            };
            g.DrawEllipse(pen, 6, 6, 20, 20);
            g.DrawString("i", font, brush, new RectangleF(0, 2, Size, Size), sf);
        }

        private static void Default(Graphics g)
        {
            using var brush = new SolidBrush(Color.FromArgb(150, 150, 150));
            g.FillRectangle(brush, 4, 4, 24, 24);
        }

        // Shared upward yield-curve path; `endY` controls the lower-left foot.
        private static PointF[] CurvePath(int endY) => new[]
        {
            new PointF(4, endY),
            new PointF(10, endY - 4),
            new PointF(16, endY - 14),
            new PointF(22, endY - 16),
            new PointF(28, endY - 18),
        };
    }
}
