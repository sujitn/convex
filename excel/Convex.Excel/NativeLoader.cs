using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using ExcelDna.Integration;

namespace Convex.Excel
{
    /// <summary>
    /// Handles loading of the native convex_ffi.dll library.
    /// This ensures the DLL is loaded from the correct directory.
    /// </summary>
    public class NativeLoader : IExcelAddIn
    {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibrary(string lpFileName);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern bool SetDllDirectory(string lpPathName);

        private static bool _initialized = false;
        private static string _loadError = null;

        public void AutoOpen()
        {
            Initialize();
        }

        public void AutoClose()
        {
            // Cleanup if needed
        }

        internal static void Initialize()
        {
            if (_initialized) return;
            _initialized = true;

            try
            {
                // Get the directory where the XLL is located
                string xllPath = ExcelDnaUtil.XllPath;
                string xllDirectory = Path.GetDirectoryName(xllPath);

                // Set the DLL search directory
                SetDllDirectory(xllDirectory);

                // Try to load the native library
                string dllPath = Path.Combine(xllDirectory, "convex_ffi.dll");

                if (!File.Exists(dllPath))
                {
                    _loadError = "convex_ffi.dll not found at: " + dllPath;
                    return;
                }

                IntPtr handle = LoadLibrary(dllPath);
                if (handle == IntPtr.Zero)
                {
                    int error = Marshal.GetLastWin32Error();
                    _loadError = "Failed to load convex_ffi.dll. Error code: " + error;
                }
            }
            catch (Exception ex)
            {
                _loadError = "Exception loading native library: " + ex.Message;
            }
        }

        internal static string GetLoadError()
        {
            return _loadError ?? "No error";
        }
    }
}
