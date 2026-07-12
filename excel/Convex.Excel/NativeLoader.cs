using System;
using System.IO;
using System.Runtime.InteropServices;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Loads convex_ffi.dll from the directory the .xll runs from.
    ///
    /// ExcelDna 1.7 cannot pack native libraries into the .xll (its .dna
    /// schema has no such element), so the DLL ships side-by-side with the
    /// add-in and this loader binds it explicitly by absolute path before any
    /// P/Invoke fires. Once loaded, later [DllImport("convex_ffi.dll")] calls
    /// resolve to the already-loaded module by name. Load status is surfaced
    /// via =CX.DIAG() and the ribbon Diagnostics button.
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

            // In-grid function tooltips (argument help while typing =CX.…),
            // the same affordance the Bloomberg add-in gives for BDP/BDH.
            try { ExcelDna.IntelliSense.IntelliSenseServer.Install(); }
            catch { /* IntelliSense is best-effort — never block the add-in */ }

            // Registry handles are process-lifetime: a saved workbook's cached
            // #CX# strings are stale in a fresh Excel. Builder cells only
            // re-register on a full rebuild, so queue one at load time (runs
            // after Excel finishes opening the workbook that launched it).
            // Workbooks opened later in the session use the ribbon Rebuild
            // button. Off-switch: Settings → "Rebuild handles on open".
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

                // Absolute-path load; deliberately NOT SetDllDirectory, which
                // mutates the process-wide search path and can break other
                // add-ins' native loads.
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
