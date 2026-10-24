using System;
using System.IO;
using System.Runtime.InteropServices;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Binds convex_ffi.dll by absolute path before any P/Invoke fires —
    /// ExcelDna 1.7 cannot pack native libraries, so the DLL ships next to
    /// the .xll. Load status: =CX.DIAG() / ribbon Diagnostics.
    /// </summary>
    public class NativeLoader : IExcelAddIn
    {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryEx(string lpFileName, IntPtr hFile, uint dwFlags);

        private const uint LoadWithAlteredSearchPath = 0x00000008;

        private static bool _initialized;
        private static string? _loadError;
        private static string? _loadedPath;

        public void AutoOpen()
        {
            Initialize();

            try { ExcelDna.IntelliSense.IntelliSenseServer.Install(); }
            catch { /* best-effort — never block the add-in */ }

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
