using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using Convex.Excel.Forms;
using Convex.Excel.Helpers;
using ExcelDna.Integration;
using ExcelDna.Integration.CustomUI;

namespace Convex.Excel
{
    [ComVisible(true)]
    public class RibbonController : ExcelRibbon
    {
        public override object LoadImage(string imageId) => IconAtlas.Get(imageId);

        // Tickets and browsers are modeless (Excel stays interactive), one
        // instance per form type; re-clicking focuses the open one.
        public void OnPricingTicket(IRibbonControl _) => ShowModeless(() => new PricingTicketForm());
        public void OnSpreadTicket(IRibbonControl _) => ShowModeless(() => new SpreadTicketForm());
        public void OnCurveViewer(IRibbonControl _) => ShowModeless(() => new CurveViewerForm());
        public void OnScenario(IRibbonControl _) => ShowModeless(() => new ScenarioForm());
        public void OnObjectBrowser(IRibbonControl _) => ShowModeless(() => new ObjectBrowserForm());
        public void OnErrorLog(IRibbonControl _) => ShowModeless(() => new ErrorLogForm());

        // Builders and settings are short-lived edit dialogs — modal is fine.
        public void OnNewBond(IRibbonControl _) => Show(() => new BondBuilderForm());
        public void OnNewCurve(IRibbonControl _) => Show(() => new CurveBuilderForm());
        public void OnSchemaBrowser(IRibbonControl _) => Show(() => new SchemaBrowserForm());
        public void OnSettings(IRibbonControl _) => Show(() => new SettingsForm());

        public void OnClearAll(IRibbonControl _)
        {
            var ok = MessageBox.Show(
                "Release every registered Convex object? Existing handles in cells will become invalid.",
                "Confirm Clear All", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
            if (ok == DialogResult.Yes) Cx.ClearAll();
        }

        public void OnRebuild(IRibbonControl _)
        {
            try
            {
                ((dynamic)ExcelDna.Integration.ExcelDnaUtil.Application).CalculateFullRebuild();
                SheetHelpers.Status("Convex: full rebuild triggered");
            }
            catch (Exception ex)
            {
                MessageBox.Show("Rebuild failed: " + ex.Message, "Convex",
                    MessageBoxButtons.OK, MessageBoxIcon.Warning);
            }
        }

        public void OnDiagnostics(IRibbonControl _)
        {
            string version;
            try { version = Cx.Version(); }
            catch (Exception ex) { version = "unavailable (" + ex.Message + ")"; }
            int objects;
            try { objects = Cx.ObjectCount(); }
            catch { objects = -1; }
            MessageBox.Show(
                "Native library: " + NativeLoader.GetLoadError() +
                "\nEngine version: " + version +
                "\nRegistered objects: " + objects +
                "\n\nPer-cell error detail: =CX.LASTERROR(cell)",
                "Convex Diagnostics", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        public void OnAbout(IRibbonControl _)
        {
            string version;
            try { version = Cx.Version(); }
            catch (Exception ex) { version = "unknown (" + ex.Message + ")"; }
            MessageBox.Show(
                "Convex Excel Add-In\n\nVersion: " + version +
                "\n\nMark-driven fixed income analytics.\n\nType =CX.SCHEMA(\"Mark\") for the wire format.",
                "About Convex", MessageBoxButtons.OK, MessageBoxIcon.Information);
        }

        private static void Show(Func<Form> factory)
        {
            try
            {
                using var form = factory();
                form.ShowDialog(ExcelOwner);
            }
            catch (Exception ex)
            {
                MessageBox.Show(ex.ToString(), "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        private static readonly Dictionary<Type, Form> _open = new();

        private static void ShowModeless<T>(Func<T> factory) where T : Form
        {
            try
            {
                if (_open.TryGetValue(typeof(T), out var existing) && !existing.IsDisposed)
                {
                    existing.Activate();
                    return;
                }
                // NOT `using` — a modeless form must outlive this handler.
                var form = factory();
                form.StartPosition = FormStartPosition.CenterScreen;
                form.FormClosed += (_, _) => _open.Remove(typeof(T));
                _open[typeof(T)] = form;
                form.Show(ExcelOwner);
            }
            catch (Exception ex)
            {
                MessageBox.Show(ex.ToString(), "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }

        // Excel-owned forms stay above the grid without being topmost.
        private static IWin32Window? ExcelOwner
        {
            get
            {
                try { return new WindowWrapper(ExcelDnaUtil.WindowHandle); }
                catch { return null; }
            }
        }

        private sealed class WindowWrapper : IWin32Window
        {
            public WindowWrapper(IntPtr handle) => Handle = handle;
            public IntPtr Handle { get; }
        }
    }
}
