using System;
using System.IO;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel
{
    // Tiny user-settings store: {DefaultFrequency, DefaultDayCount, DefaultSpreadType}.
    // Loaded lazily, written on Save(). Lives at %APPDATA%/Convex/settings.json.
    internal static class CxSettings
    {
        public sealed class Snapshot
        {
            public string DefaultFrequency { get; set; } = "SemiAnnual";
            public string DefaultDayCount { get; set; } = "Thirty360US";
            public string DefaultSpreadType { get; set; } = "Z";
            public string DefaultCurrency { get; set; } = "USD";
        }

        private static readonly object _lock = new();
        private static Snapshot? _cached;

        private static string Path
        {
            get
            {
                var dir = System.IO.Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData),
                    "Convex");
                Directory.CreateDirectory(dir);
                return System.IO.Path.Combine(dir, "settings.json");
            }
        }

        public static Snapshot Current
        {
            get
            {
                lock (_lock)
                {
                    if (_cached != null) return _cached;
                    _cached = Load() ?? new Snapshot();
                    return _cached;
                }
            }
        }

        private static Snapshot? Load()
        {
            try
            {
                if (!File.Exists(Path)) return null;
                var node = JToken.Parse(File.ReadAllText(Path)) as JObject;
                if (node == null) return null;
                return new Snapshot
                {
                    DefaultFrequency = (string?)node["DefaultFrequency"] ?? "SemiAnnual",
                    DefaultDayCount = (string?)node["DefaultDayCount"] ?? "Thirty360US",
                    DefaultSpreadType = (string?)node["DefaultSpreadType"] ?? "Z",
                    DefaultCurrency = (string?)node["DefaultCurrency"] ?? "USD",
                };
            }
            catch { return null; }
        }

        public static void Save(Snapshot snap)
        {
            lock (_lock)
            {
                var node = new JObject
                {
                    ["DefaultFrequency"] = snap.DefaultFrequency,
                    ["DefaultDayCount"] = snap.DefaultDayCount,
                    ["DefaultSpreadType"] = snap.DefaultSpreadType,
                    ["DefaultCurrency"] = snap.DefaultCurrency,
                };
                File.WriteAllText(Path, node.ToString(Formatting.None));
                _cached = snap;
            }
        }
    }
}
