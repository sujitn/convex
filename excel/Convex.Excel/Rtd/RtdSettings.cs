using System;
using System.Threading;

namespace Convex.Excel.Rtd
{
    /// <summary>
    /// Global RTD settings and state management.
    /// Controls update intervals, enable/disable, and server lifecycle.
    /// </summary>
    public static class RtdSettings
    {
        private static bool _enabled = true;
        private static int _updateIntervalMs = 100;
        private static int _throttleMs = 500;
        private static bool _autoStart = true;
        private static readonly object _lock = new object();

        /// <summary>
        /// Event raised when settings change.
        /// </summary>
        public static event EventHandler SettingsChanged;

        /// <summary>
        /// Gets or sets whether RTD updates are enabled.
        /// When disabled, functions return cached values without updating.
        /// </summary>
        public static bool Enabled
        {
            get { lock (_lock) return _enabled; }
            set
            {
                lock (_lock)
                {
                    if (_enabled != value)
                    {
                        _enabled = value;
                        OnSettingsChanged();
                    }
                }
            }
        }

        /// <summary>
        /// Gets or sets the minimum interval between RTD updates (milliseconds).
        /// Lower values = more responsive but higher CPU usage.
        /// Default: 100ms
        /// </summary>
        public static int UpdateIntervalMs
        {
            get { lock (_lock) return _updateIntervalMs; }
            set
            {
                lock (_lock)
                {
                    value = Math.Max(50, Math.Min(5000, value)); // Clamp 50-5000ms
                    if (_updateIntervalMs != value)
                    {
                        _updateIntervalMs = value;
                        TopicManager.SetUpdateInterval(value);
                        OnSettingsChanged();
                    }
                }
            }
        }

        /// <summary>
        /// Gets or sets the throttle time for batching updates (milliseconds).
        /// Updates within this window are batched together.
        /// Default: 500ms
        /// </summary>
        public static int ThrottleMs
        {
            get { lock (_lock) return _throttleMs; }
            set
            {
                lock (_lock)
                {
                    value = Math.Max(100, Math.Min(10000, value)); // Clamp 100-10000ms
                    if (_throttleMs != value)
                    {
                        _throttleMs = value;
                        OnSettingsChanged();
                    }
                }
            }
        }

        /// <summary>
        /// Gets or sets whether RTD server starts automatically.
        /// </summary>
        public static bool AutoStart
        {
            get { lock (_lock) return _autoStart; }
            set
            {
                lock (_lock)
                {
                    _autoStart = value;
                    OnSettingsChanged();
                }
            }
        }

        /// <summary>
        /// Gets whether the RTD server is currently running.
        /// </summary>
        public static bool IsServerRunning => ConvexRtdServer.Instance != null;

        /// <summary>
        /// Gets the number of active RTD topics.
        /// </summary>
        public static int ActiveTopicCount
        {
            get
            {
                var server = ConvexRtdServer.Instance;
                if (server == null) return 0;
                var stats = server.GetStats();
                return stats.totalTopics;
            }
        }

        /// <summary>
        /// Gets RTD statistics.
        /// </summary>
        public static (int total, int curves, int bonds, int analytics) GetStatistics()
        {
            var server = ConvexRtdServer.Instance;
            if (server == null) return (0, 0, 0, 0);
            return server.GetStats();
        }

        /// <summary>
        /// Pauses all RTD updates temporarily.
        /// </summary>
        public static void Pause()
        {
            Enabled = false;
        }

        /// <summary>
        /// Resumes RTD updates.
        /// </summary>
        public static void Resume()
        {
            Enabled = true;
        }

        /// <summary>
        /// Forces an immediate refresh of all RTD topics.
        /// </summary>
        public static void RefreshAll()
        {
            TopicManager.NotifyChange("");
        }

        /// <summary>
        /// Resets settings to defaults.
        /// </summary>
        public static void ResetToDefaults()
        {
            lock (_lock)
            {
                _enabled = true;
                _updateIntervalMs = 100;
                _throttleMs = 500;
                _autoStart = true;
                TopicManager.SetUpdateInterval(_updateIntervalMs);
                OnSettingsChanged();
            }
        }

        private static void OnSettingsChanged()
        {
            SettingsChanged?.Invoke(null, EventArgs.Empty);
        }

        /// <summary>
        /// Preset configurations for common use cases.
        /// </summary>
        public static class Presets
        {
            /// <summary>
            /// High-frequency trading: 50ms updates, minimal throttling
            /// </summary>
            public static void HighFrequency()
            {
                UpdateIntervalMs = 50;
                ThrottleMs = 100;
            }

            /// <summary>
            /// Normal trading: 200ms updates, moderate throttling
            /// </summary>
            public static void Normal()
            {
                UpdateIntervalMs = 200;
                ThrottleMs = 500;
            }

            /// <summary>
            /// Low frequency/research: 1000ms updates, high throttling
            /// </summary>
            public static void LowFrequency()
            {
                UpdateIntervalMs = 1000;
                ThrottleMs = 2000;
            }

            /// <summary>
            /// Battery saver: 2000ms updates, maximum throttling
            /// </summary>
            public static void BatterySaver()
            {
                UpdateIntervalMs = 2000;
                ThrottleMs = 5000;
            }
        }
    }
}
