using System;
using System.Runtime.InteropServices;
using System.Windows.Forms;
using Convex.Excel.Forms;
using Convex.Excel.Helpers;
using ExcelDna.Integration.CustomUI;

namespace Convex.Excel
{
    [ComVisible(true)]
    public class RibbonController : ExcelRibbon
    {
        public override object LoadImage(string imageId) => IconAtlas.Get(imageId);

        public void OnNewBond(IRibbonControl _) => Show(() => new BondBuilderForm());
        public void OnPricingTicket(IRibbonControl _) => Show(() => new PricingTicketForm());
        public void OnSpreadTicket(IRibbonControl _) => Show(() => new SpreadTicketForm());
        public void OnNewCurve(IRibbonControl _) => Show(() => new CurveBuilderForm());
        public void OnCurveViewer(IRibbonControl _) => Show(() => new CurveViewerForm());
        public void OnScenario(IRibbonControl _) => Show(() => new ScenarioForm());
        public void OnObjectBrowser(IRibbonControl _) => Show(() => new ObjectBrowserForm());
        public void OnSchemaBrowser(IRibbonControl _) => Show(() => new SchemaBrowserForm());
        public void OnSettings(IRibbonControl _) => Show(() => new SettingsForm());

        public void OnClearAll(IRibbonControl _)
        {
            var ok = MessageBox.Show(
                "Release every registered Convex object? Existing handles in cells will become invalid.",
                "Confirm Clear All", MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
            if (ok == DialogResult.Yes) Cx.ClearAll();
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
                form.ShowDialog();
            }
            catch (Exception ex)
            {
                MessageBox.Show(ex.ToString(), "Error", MessageBoxButtons.OK, MessageBoxIcon.Error);
            }
        }
    }
}
