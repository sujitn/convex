using System;
using System.IO;
using System.Runtime.InteropServices;
using Convex.Excel.Ops;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Binds convex_ffi.dll by absolute path before any P/Invoke fires —
    /// a packed native library fails to load on Excel-DNA 1.9 / net472, so
    /// the DLL ships next to the .xll. Load status: =CX.DIAG() / ribbon
    /// Diagnostics.
    /// </summary>
    public class NativeLoader : IExcelAddIn
    {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryEx(string lpFileName, IntPtr hFile, uint dwFlags);

        private const uint LoadWithAlteredSearchPath = 0x00000008;

        private static bool _initialized;
        private static string? _loadError;
        private static string? _loadedPath;

        /// The workspace version the add-in was built at (major.minor.patch).
        internal static string AddInVersion =>
            typeof(NativeLoader).Assembly.GetName().Version?.ToString(3) ?? "?";

        public void AutoOpen()
        {
            Initialize();

            try
            {
                CxLog.Start();
                CxLog.Info(StartLine());
            }
            catch (Exception ex) { CxErrorStore.Record(ex, "(log)", "log file"); }

            try { ExcelDna.IntelliSense.IntelliSenseServer.Install(); }
            catch { /* best-effort — never block the add-in */ }

            // The server link (CX.LIVE): load %APPDATA%\Convex\server.json.
            // A single sign-on link starts signed out until Sign in.
            try { Server.ServerLink.Start(); }
            catch (Exception ex) { CxErrorStore.Record(ex, "(server)", "server link"); }

            // Handles are process-lifetime, so a reopened workbook's cached
            // #CX# strings are stale until builder cells re-register. The
            // queued rebuild runs after the launching workbook has opened;
            // workbooks opened later in the session use the ribbon Rebuild.
            if (CxSettings.Current.AutoRebuildOnOpen)
            {
                ExcelAsyncUtil.QueueAsMacro(() =>
                {
                    try { ((dynamic)ExcelDnaUtil.Application).CalculateFullRebuild(); }
                    catch { /* best effort — a blank session has nothing to rebuild */ }
                });
            }
        }

        public void AutoClose()
        {
            try { ExcelDna.IntelliSense.IntelliSenseServer.Uninstall(); }
            catch { }
            try { Server.ServerLink.Source.Dispose(); }
            catch { }
            try
            {
                CxLog.Info("add-in removed");
                CxLog.Stop();
            }
            catch { }
        }

        /// Excel is exiting. The ribbon's COM add-in hears this; AutoClose runs
        /// only when the add-in is removed. The log's writer has flushed what
        /// it holds whenever its queue is empty, so the file needs no closing.
        internal static void ExcelClosing() => CxLog.Info("Excel closing");

        private static string StartLine()
        {
            string engine, build;
            try { engine = Cx.Version(); }
            catch (Exception ex) { engine = "unavailable: " + ex.Message; }
            try { build = Convert.ToString(((dynamic)ExcelDnaUtil.Application).Build, System.Globalization.CultureInfo.InvariantCulture); }
            catch (Exception) { build = "?"; }
            return $"add-in {AddInVersion} loaded; engine {engine}; Excel {ExcelDnaUtil.ExcelVersion:0.0} build {build}; " +
                   $"{ExcelDnaUtil.XllPath}; native library: {GetLoadError()}";
        }

        internal static void Initialize()
        {
            if (_initialized) return;
            _initialized = true;

            try
            {
                string xllDirectory = Path.GetDirectoryName(ExcelDnaUtil.XllPath) ?? "";
                string dllPath = Path.Combine(xllDirectory, "convex_ffi.dll");

                if (!File.Exists(dllPath))
                {
                    _loadError = "convex_ffi.dll not found at: " + dllPath +
                                 " — it must sit in the same folder as the .xll";
                    return;
                }

                // NOT SetDllDirectory — that mutates the process-wide search
                // path and can break other add-ins' native loads.
                IntPtr handle = LoadLibraryEx(dllPath, IntPtr.Zero, LoadWithAlteredSearchPath);
                if (handle == IntPtr.Zero)
                {
                    int error = Marshal.GetLastWin32Error();
                    _loadError = $"Failed to load {dllPath}. Win32 error code: {error}";
                }
                else
                {
                    _loadedPath = dllPath;
                }
            }
            catch (Exception ex)
            {
                _loadError = "Exception loading native library: " + ex.Message;
            }
        }

        internal static string GetLoadError()
        {
            if (_loadError != null) return _loadError;
            if (_loadedPath != null) return "OK — " + _loadedPath;
            return "not initialized";
        }
    }
}
