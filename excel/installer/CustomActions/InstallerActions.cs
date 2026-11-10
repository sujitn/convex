using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using Microsoft.Win32;
using WixToolset.Dtf.WindowsInstaller;

namespace Convex.Excel.Installer
{
    /// The installer's custom actions. The edits themselves are in
    /// OpenRegistration; this class only reads and writes the registry.
    public static class InstallerActions
    {
        private const string OptionsKey = @"Software\Microsoft\Office\16.0\Excel\Options";

        /// Excel holds the .xll open, and it may write the OPEN values back when
        /// it exits, so Package.wxs refuses to change anything while it runs.
        [CustomAction]
        public static ActionResult SetExcelRunning(Session session)
        {
            session["EXCEL_RUNNING"] = Process.GetProcessesByName("EXCEL").Length > 0 ? "1" : "";
            return ActionResult.Success;
        }

        /// Immediate: records the values for RestoreOpen, which is deferred
        /// and so can read only its own property.
        [CustomAction]
        public static ActionResult SnapshotOpen(Session session)
        {
            var snapshot = new CustomActionData();
            foreach (var value in Read()) snapshot[value.Key] = value.Value;
            session["RestoreOpen"] = snapshot.ToString();
            return ActionResult.Success;
        }

        [CustomAction]
        public static ActionResult RegisterOpen(Session session)
        {
            var data = session.CustomActionData;
            Write(session, OpenRegistration.Register(Read(), data["XLL"], data["ROOT"]));
            return ActionResult.Success;
        }

        [CustomAction]
        public static ActionResult UnregisterOpen(Session session)
        {
            Write(session, OpenRegistration.Unregister(Read(), session.CustomActionData["ROOT"]));
            return ActionResult.Success;
        }

        /// Rollback: puts back the values SnapshotOpen recorded.
        [CustomAction]
        public static ActionResult RestoreOpen(Session session)
        {
            Write(session, session.CustomActionData.ToDictionary(v => v.Key, v => v.Value, StringComparer.OrdinalIgnoreCase));
            return ActionResult.Success;
        }

        private static Dictionary<string, string> Read()
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            using var key = Registry.CurrentUser.OpenSubKey(OptionsKey);
            if (key == null) return values;
            foreach (var name in key.GetValueNames().Where(OpenRegistration.IsOpenValue))
                if (key.GetValue(name) is string value) values[name] = value;
            return values;
        }

        private static void Write(Session session, IReadOnlyDictionary<string, string> wanted)
        {
            using var key = Registry.CurrentUser.CreateSubKey(OptionsKey);
            foreach (var name in key.GetValueNames().Where(OpenRegistration.IsOpenValue))
                if (!wanted.ContainsKey(name)) key.DeleteValue(name, throwOnMissingValue: false);
            foreach (var value in wanted) key.SetValue(value.Key, value.Value, RegistryValueKind.String);
            session.Log("Convex: OPEN values are now: " + string.Join(", ", wanted.Select(v => v.Key + "=" + v.Value)));
        }
    }
}
