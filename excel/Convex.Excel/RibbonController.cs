using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;
using Convex.Excel.Forms;
using Convex.Excel.Helpers;
using Convex.Excel.Server;
using LinkState = Convex.Excel.Server.LinkState;
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
        public void OnLimitTicket(IRibbonControl _) => ShowModeless(() => new LimitTicketForm());

        // Builders and settings are short-lived edit dialogs — modal is fine.
        public void OnNewBond(IRibbonControl _) => Show(() => new BondBuilderForm());
        public void OnNewCurve(IRibbonControl _) => Show(() => new CurveBuilderForm());
        public void OnSchemaBrowser(IRibbonControl _) => Show(() => new SchemaBrowserForm());
        public void OnSettings(IRibbonControl _) => Show(() => new SettingsForm());

        // ---- Server link: an indicator that follows the connection, and sign-in.
        private static IRibbonUI? _ribbon;

        public void OnRibbonLoad(IRibbonUI ribbon)
        {
            _ribbon = ribbon;
            ServerLink.Changed += () =>
            {
                // Ribbon calls belong on Excel's main thread.
                try { ExcelAsyncUtil.QueueAsMacro(() => _ribbon?.Invalidate()); }
                catch { /* Excel is closing */ }
            };
        }

        public string GetServerLabel(IRibbonControl _) => ServerLink.Source.Status().Link switch
        {
            LinkState.Connected => "Connected",
            LinkState.Connecting => "Connecting",
            LinkState.Reconnecting => "Reconnecting",
            LinkState.SignedOut => "Signed out",
            _ => "No server",
        };

        public string GetServerTip(IRibbonControl _)
        {
            var s = ServerLink.Source.Status();
            return s.Connection + (s.Server.Length > 0 ? "\n" + s.Server : "") + "\n" + s.User + "\nCalculation: " + CalculationMode();
        }

        public bool GetSignInEnabled(IRibbonControl _) => ServerLink.CanSignIn;
        public bool GetSignOutEnabled(IRibbonControl _) => ServerLink.CanSignOut;

        public void OnServerSettings(IRibbonControl _) => Show(() => new ServerSettingsForm());

        public void OnServerStatus(IRibbonControl _)
        {
            var s = ServerLink.Source.Status();
            var text =
                "Connection: " + s.Connection +
                "\nServer: " + (s.Server.Length > 0 ? s.Server : "(none)") +
                "\nUser: " + s.User +
                "\nRuntime: " + (s.Runtime ?? "(none yet)") +
                "\nSession: " + (s.SessionId ?? "(none)") +
                "\nInstruments followed: " + s.Instruments +
                "\nLast message: " + (s.LastMessageUtc?.ToLocalTime().ToString("T") ?? "(none)") +
                (s.Problem != null ? "\nLast server error: " + s.Problem : "") +
                "\nCalculation: " + CalculationMode() +
                "\n\nLive cells update by themselves only under Automatic calculation." +
                "\n\nReconnect now? (Refused instruments are tried again.)";
            if (MessageBox.Show(text, "Convex server", MessageBoxButtons.YesNo, MessageBoxIcon.Information) == DialogResult.Yes)
                ServerLink.Source.Reconnect();
        }

        public void OnSignIn(IRibbonControl _)
        {
            SheetHelpers.Status("Convex: sign in in your browser…");
            Task.Run(async () =>
            {
                try
                {
                    await ServerLink.SignInAsync(CancellationToken.None).ConfigureAwait(false);
                    ExcelAsyncUtil.QueueAsMacro(() => SheetHelpers.Status("Convex: signed in"));
                }
                catch (Exception ex)
                {
                    ExcelAsyncUtil.QueueAsMacro(() =>
                    {
                        SheetHelpers.Status("");
                        MessageBox.Show("Sign-in failed: " + ex.Message, "Convex",
                            MessageBoxButtons.OK, MessageBoxIcon.Warning);
                    });
                }
            });
        }

        public void OnSignOut(IRibbonControl _) => ServerLink.SignOut();

        private static string CalculationMode()
        {
            try
            {
                int mode = (int)((dynamic)ExcelDnaUtil.Application).Calculation;
                return mode == -4105 ? "Automatic" : mode == -4135 ? "Manual" : "Automatic except tables";
            }
            catch
            {
                return "unknown";
            }
        }

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
                "Convex Excel Add-In\n\nAdd-in: " + NativeLoader.AddInVersion +
                "\nEngine: " + version +
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
