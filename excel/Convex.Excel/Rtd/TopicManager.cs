using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Threading;

namespace Convex.Excel.Rtd
{
    /// <summary>
    /// Manages RTD topics, their dependencies, and triggers recalculation.
    /// Implements a dependency graph for efficient cascade updates.
    /// </summary>
    public static class TopicManager
    {
        // Active subscriptions by topic string
        private static readonly ConcurrentDictionary<string, HashSet<RtdTopic>> _subscriptions
            = new ConcurrentDictionary<string, HashSet<RtdTopic>>();

        // Dependency graph: topic -> topics that depend on it
        private static readonly ConcurrentDictionary<string, HashSet<string>> _dependents
            = new ConcurrentDictionary<string, HashSet<string>>();

        // Cached calculation results
        private static readonly ConcurrentDictionary<string, CachedValue> _cache
            = new ConcurrentDictionary<string, CachedValue>();

        // Object handles by topic (for curve/bond topics)
        private static readonly ConcurrentDictionary<string, ulong> _handles
            = new ConcurrentDictionary<string, ulong>();

        private static readonly object _lock = new object();
        private static Timer _updateTimer;
        private static readonly HashSet<string> _pendingUpdates = new HashSet<string>();

        /// <summary>
        /// Starts the topic manager timer.
        /// </summary>
        internal static void Start()
        {
            // Start update timer (throttles updates to prevent calculation storms)
            if (_updateTimer == null)
                _updateTimer = new Timer(ProcessPendingUpdates, null, 100, 100);
        }

        /// <summary>
        /// Shuts down the topic manager.
        /// </summary>
        public static void Shutdown()
        {
            _updateTimer?.Dispose();
            _updateTimer = null;
            _subscriptions.Clear();
            _dependents.Clear();
            _cache.Clear();
            _handles.Clear();
        }

        /// <summary>
        /// Sets the update interval for the timer.
        /// </summary>
        public static void SetUpdateInterval(int intervalMs)
        {
            _updateTimer?.Change(intervalMs, intervalMs);
        }

        /// <summary>
        /// Subscribes an RTD topic.
        /// </summary>
        public static void Subscribe(RtdTopic topic)
        {
            var (type, name, parameters) = topic.Parse();
            if (string.IsNullOrEmpty(type)) return;

            // Add to subscriptions
            var subs = _subscriptions.GetOrAdd(topic.TopicString, _ => new HashSet<RtdTopic>());
            lock (subs)
            {
                subs.Add(topic);
            }

            // Register dependencies based on topic type
            RegisterDependencies(topic);
        }

        /// <summary>
        /// Unsubscribes an RTD topic.
        /// </summary>
        public static void Unsubscribe(RtdTopic topic)
        {
            if (_subscriptions.TryGetValue(topic.TopicString, out var subs))
            {
                lock (subs)
                {
                    subs.Remove(topic);
                    if (subs.Count == 0)
                    {
                        _subscriptions.TryRemove(topic.TopicString, out _);
                        // Clean up dependencies
                        UnregisterDependencies(topic);
                    }
                }
            }
        }

        /// <summary>
        /// Calculates the value for a topic.
        /// </summary>
        public static object CalculateValue(string topicString)
        {
            var parts = topicString.Split(':');
            if (parts.Length < 2)
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;

            string type = parts[0].ToLowerInvariant();
            string name = parts[1];

            try
            {
                switch (type)
                {
                    case "curve":
                        return CalculateCurve(name, parts.Skip(2).ToArray());

                    case "bond":
                        return CalculateBond(name, parts.Skip(2).ToArray());

                    case "yield":
                        return CalculateYield(parts);

                    case "price":
                        return CalculatePrice(parts);

                    case "duration":
                        return CalculateDuration(parts);

                    case "zspread":
                        return CalculateZSpread(parts);

                    case "handle":
                        // Return the handle for a named object
                        if (_handles.TryGetValue($"{parts[1]}:{parts[2]}", out var handle))
                            return HandleHelper.Format(handle);
                        return ExcelDna.Integration.ExcelError.ExcelErrorRef;

                    default:
                        return ExcelDna.Integration.ExcelError.ExcelErrorValue;
                }
            }
            catch (Exception ex)
            {
                return $"#ERROR: {ex.Message}";
            }
        }

        /// <summary>
        /// Notifies that a topic's inputs have changed.
        /// </summary>
        public static void NotifyChange(string topicPattern)
        {
            lock (_pendingUpdates)
            {
                _pendingUpdates.Add(topicPattern);
            }
        }

        /// <summary>
        /// Processes pending updates (called by timer).
        /// </summary>
        private static void ProcessPendingUpdates(object state)
        {
            HashSet<string> updates;
            lock (_pendingUpdates)
            {
                if (_pendingUpdates.Count == 0) return;
                updates = new HashSet<string>(_pendingUpdates);
                _pendingUpdates.Clear();
            }

            var server = ConvexRtdServer.Instance;
            if (server == null) return;

            // Find all affected topics and recalculate
            var affectedTopics = new HashSet<string>();
            foreach (var pattern in updates)
            {
                // Add direct matches
                foreach (var topicString in _subscriptions.Keys)
                {
                    if (topicString.StartsWith(pattern, StringComparison.OrdinalIgnoreCase) ||
                        topicString.Contains($":{pattern}:") ||
                        topicString.EndsWith($":{pattern}"))
                    {
                        affectedTopics.Add(topicString);
                    }
                }

                // Add dependents
                if (_dependents.TryGetValue(pattern, out var deps))
                {
                    foreach (var dep in deps)
                    {
                        affectedTopics.Add(dep);
                    }
                }
            }

            // Recalculate and update affected topics
            foreach (var topicString in affectedTopics)
            {
                if (_subscriptions.TryGetValue(topicString, out var subs))
                {
                    var newValue = CalculateValue(topicString);
                    lock (subs)
                    {
                        foreach (var topic in subs)
                        {
                            server.UpdateTopic(topic, newValue);
                        }
                    }
                }
            }
        }

        /// <summary>
        /// Registers dependencies for a topic.
        /// </summary>
        private static void RegisterDependencies(RtdTopic topic)
        {
            var (type, name, parameters) = topic.Parse();

            // Analytics topics depend on their curve/bond inputs
            switch (type.ToLowerInvariant())
            {
                case "yield":
                case "price":
                case "duration":
                case "convexity":
                case "dv01":
                    // Format: "yield:bondName:curveName:..."
                    if (parameters.Length >= 1)
                    {
                        string bondKey = $"bond:{name}";
                        AddDependency(bondKey, topic.TopicString);
                        topic.Dependencies.Add(bondKey);

                        if (parameters.Length >= 1)
                        {
                            string curveKey = $"curve:{parameters[0]}";
                            AddDependency(curveKey, topic.TopicString);
                            topic.Dependencies.Add(curveKey);
                        }
                    }
                    break;

                case "zspread":
                case "ispread":
                case "gspread":
                case "asw":
                    // Format: "zspread:bondName:curveName:..."
                    if (parameters.Length >= 1)
                    {
                        string bondKey = $"bond:{name}";
                        AddDependency(bondKey, topic.TopicString);
                        topic.Dependencies.Add(bondKey);

                        string curveKey = $"curve:{parameters[0]}";
                        AddDependency(curveKey, topic.TopicString);
                        topic.Dependencies.Add(curveKey);
                    }
                    break;
            }
        }

        /// <summary>
        /// Unregisters dependencies for a topic.
        /// </summary>
        private static void UnregisterDependencies(RtdTopic topic)
        {
            foreach (var dep in topic.Dependencies)
            {
                if (_dependents.TryGetValue(dep, out var deps))
                {
                    lock (deps)
                    {
                        deps.Remove(topic.TopicString);
                    }
                }
            }
        }

        /// <summary>
        /// Adds a dependency relationship.
        /// </summary>
        private static void AddDependency(string source, string dependent)
        {
            var deps = _dependents.GetOrAdd(source, _ => new HashSet<string>());
            lock (deps)
            {
                deps.Add(dependent);
            }
        }

        /// <summary>
        /// Stores a handle for a named object.
        /// </summary>
        public static void StoreHandle(string type, string name, ulong handle)
        {
            _handles[$"{type}:{name}"] = handle;
        }

        /// <summary>
        /// Gets a handle for a named object.
        /// </summary>
        public static ulong? GetHandle(string type, string name)
        {
            if (_handles.TryGetValue($"{type}:{name}", out var handle))
                return handle;
            return null;
        }

        /// <summary>
        /// Generates a hash for input parameters (for cache invalidation).
        /// </summary>
        public static string HashInputs(params object[] inputs)
        {
            var sb = new StringBuilder();
            foreach (var input in inputs)
            {
                if (input is object[,] array)
                {
                    for (int i = 0; i < array.GetLength(0); i++)
                        for (int j = 0; j < array.GetLength(1); j++)
                            sb.Append(array[i, j]?.ToString() ?? "null").Append("|");
                }
                else if (input is object[] arr)
                {
                    foreach (var item in arr)
                        sb.Append(item?.ToString() ?? "null").Append("|");
                }
                else
                {
                    sb.Append(input?.ToString() ?? "null").Append("|");
                }
            }

            using (var sha = SHA256.Create())
            {
                var hash = sha.ComputeHash(Encoding.UTF8.GetBytes(sb.ToString()));
                return Convert.ToBase64String(hash).Substring(0, 8);
            }
        }

        #region Calculation Methods

        private static object CalculateCurve(string name, string[] parameters)
        {
            // The curve should already be created by the RTD function
            // Just return the handle
            if (_handles.TryGetValue($"curve:{name}", out var handle))
            {
                return HandleHelper.Format(handle);
            }
            return ExcelDna.Integration.ExcelError.ExcelErrorRef;
        }

        private static object CalculateBond(string name, string[] parameters)
        {
            if (_handles.TryGetValue($"bond:{name}", out var handle))
            {
                return HandleHelper.Format(handle);
            }
            return ExcelDna.Integration.ExcelError.ExcelErrorRef;
        }

        private static object CalculateYield(string[] parts)
        {
            // Format: "yield:bondName:settlement:price:freq"
            if (parts.Length < 5) return ExcelDna.Integration.ExcelError.ExcelErrorValue;

            string bondName = parts[1];
            if (!DateTime.TryParse(parts[2], out var settlement))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!double.TryParse(parts[3], out var price))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!int.TryParse(parts[4], out var freq))
                freq = 2;

            if (!_handles.TryGetValue($"bond:{bondName}", out var bondHandle))
                return ExcelDna.Integration.ExcelError.ExcelErrorRef;

            return ConvexWrapper.CalculateYield(bondHandle, settlement, price, freq);
        }

        private static object CalculatePrice(string[] parts)
        {
            // Format: "price:bondName:settlement:yield:freq"
            if (parts.Length < 5) return ExcelDna.Integration.ExcelError.ExcelErrorValue;

            string bondName = parts[1];
            if (!DateTime.TryParse(parts[2], out var settlement))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!double.TryParse(parts[3], out var yld))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!int.TryParse(parts[4], out var freq))
                freq = 2;

            if (!_handles.TryGetValue($"bond:{bondName}", out var bondHandle))
                return ExcelDna.Integration.ExcelError.ExcelErrorRef;

            return ConvexWrapper.CalculatePrice(bondHandle, settlement, yld / 100.0, freq);
        }

        private static object CalculateDuration(string[] parts)
        {
            // Format: "duration:bondName:settlement:price:freq"
            if (parts.Length < 5) return ExcelDna.Integration.ExcelError.ExcelErrorValue;

            string bondName = parts[1];
            if (!DateTime.TryParse(parts[2], out var settlement))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!double.TryParse(parts[3], out var price))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!int.TryParse(parts[4], out var freq))
                freq = 2;

            if (!_handles.TryGetValue($"bond:{bondName}", out var bondHandle))
                return ExcelDna.Integration.ExcelError.ExcelErrorRef;

            // First calculate yield from price, then use yield for duration
            double yield = ConvexWrapper.CalculateYield(bondHandle, settlement, price, freq);
            return ConvexWrapper.CalculateModifiedDuration(bondHandle, settlement, yield, freq);
        }

        private static object CalculateZSpread(string[] parts)
        {
            // Format: "zspread:bondName:curveName:settlement:price"
            if (parts.Length < 5) return ExcelDna.Integration.ExcelError.ExcelErrorValue;

            string bondName = parts[1];
            string curveName = parts[2];
            if (!DateTime.TryParse(parts[3], out var settlement))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;
            if (!double.TryParse(parts[4], out var price))
                return ExcelDna.Integration.ExcelError.ExcelErrorValue;

            if (!_handles.TryGetValue($"bond:{bondName}", out var bondHandle))
                return ExcelDna.Integration.ExcelError.ExcelErrorRef;
            if (!_handles.TryGetValue($"curve:{curveName}", out var curveHandle))
                return ExcelDna.Integration.ExcelError.ExcelErrorRef;

            return ConvexWrapper.CalculateZSpread(bondHandle, curveHandle, settlement, price);
        }

        #endregion
    }

    /// <summary>
    /// Cached calculation result.
    /// </summary>
    internal class CachedValue
    {
        public object Value { get; set; }
        public DateTime Timestamp { get; set; }
        public string InputHash { get; set; }
    }
}
