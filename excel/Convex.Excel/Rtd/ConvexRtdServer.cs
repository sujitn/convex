using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using ExcelDna.Integration;
using ExcelDna.Integration.Rtd;

namespace Convex.Excel.Rtd
{
    /// <summary>
    /// RTD Server for Convex real-time calculations.
    /// Enables efficient streaming updates for curves, bonds, and analytics.
    ///
    /// Topic format: "type:name:inputHash"
    /// Examples:
    ///   - "curve:USD.SWAP:abc123"
    ///   - "yield:AAPL.BOND:USD.SWAP:def456"
    ///   - "price:AAPL.BOND:USD.SWAP:ghi789"
    /// </summary>
    [ComVisible(true)]
    [ProgId("Convex.RtdServer")]
    public class ConvexRtdServer : ExcelRtdServer
    {
        private static ConvexRtdServer _instance;
        private readonly Dictionary<int, RtdTopic> _topics = new Dictionary<int, RtdTopic>();
        private readonly object _lock = new object();

        /// <summary>
        /// Gets the singleton instance of the RTD server.
        /// </summary>
        public static ConvexRtdServer Instance => _instance;

        /// <summary>
        /// Called when the RTD server starts.
        /// </summary>
        protected override bool ServerStart()
        {
            _instance = this;
            TopicManager.Start();
            return true;
        }

        /// <summary>
        /// Called when the RTD server terminates.
        /// </summary>
        protected override void ServerTerminate()
        {
            lock (_lock)
            {
                _topics.Clear();
            }
            TopicManager.Shutdown();
            _instance = null;
        }

        /// <summary>
        /// Called when Excel connects to a topic.
        /// </summary>
        protected override object ConnectData(Topic topic, IList<string> topicInfo, ref bool newValues)
        {
            if (topicInfo == null || topicInfo.Count == 0)
                return ExcelError.ExcelErrorValue;

            string topicString = topicInfo[0];

            lock (_lock)
            {
                var rtdTopic = new RtdTopic(topic, topicString);
                _topics[topic.TopicId] = rtdTopic;

                // Register with topic manager
                TopicManager.Subscribe(rtdTopic);

                // Calculate initial value
                object value = TopicManager.CalculateValue(topicString);
                rtdTopic.LastValue = value;

                newValues = true;
                return value;
            }
        }

        /// <summary>
        /// Called when Excel disconnects from a topic.
        /// </summary>
        protected override void DisconnectData(Topic topic)
        {
            lock (_lock)
            {
                if (_topics.TryGetValue(topic.TopicId, out var rtdTopic))
                {
                    TopicManager.Unsubscribe(rtdTopic);
                    _topics.Remove(topic.TopicId);
                }
            }
        }

        /// <summary>
        /// Updates a topic with a new value and notifies Excel.
        /// Called by TopicManager when dependencies change.
        /// </summary>
        internal void UpdateTopic(RtdTopic rtdTopic, object newValue)
        {
            if (rtdTopic?.Topic == null) return;

            lock (_lock)
            {
                if (!Equals(rtdTopic.LastValue, newValue))
                {
                    rtdTopic.LastValue = newValue;
                    rtdTopic.Topic.UpdateValue(newValue);
                }
            }
        }

        /// <summary>
        /// Forces recalculation of all topics matching a pattern.
        /// </summary>
        public void InvalidateTopics(string pattern)
        {
            lock (_lock)
            {
                foreach (var kvp in _topics)
                {
                    if (kvp.Value.TopicString.StartsWith(pattern, StringComparison.OrdinalIgnoreCase))
                    {
                        var newValue = TopicManager.CalculateValue(kvp.Value.TopicString);
                        UpdateTopic(kvp.Value, newValue);
                    }
                }
            }
        }

        /// <summary>
        /// Gets statistics about active topics.
        /// </summary>
        public (int totalTopics, int curveTopics, int bondTopics, int analyticsTopics) GetStats()
        {
            lock (_lock)
            {
                int curves = 0, bonds = 0, analytics = 0;
                foreach (var topic in _topics.Values)
                {
                    if (topic.TopicString.StartsWith("curve:")) curves++;
                    else if (topic.TopicString.StartsWith("bond:")) bonds++;
                    else analytics++;
                }
                return (_topics.Count, curves, bonds, analytics);
            }
        }
    }

    /// <summary>
    /// Represents an RTD topic subscription.
    /// </summary>
    public class RtdTopic
    {
        public ExcelRtdServer.Topic Topic { get; }
        public string TopicString { get; }
        public object LastValue { get; set; }
        public DateTime LastUpdate { get; set; }
        public HashSet<string> Dependencies { get; } = new HashSet<string>();

        public RtdTopic(ExcelRtdServer.Topic topic, string topicString)
        {
            Topic = topic;
            TopicString = topicString;
            LastUpdate = DateTime.UtcNow;
        }

        /// <summary>
        /// Parses the topic string into components.
        /// Format: "type:name:param1:param2:..."
        /// </summary>
        public (string type, string name, string[] parameters) Parse()
        {
            var parts = TopicString.Split(':');
            if (parts.Length < 2)
                return (null, null, Array.Empty<string>());

            string type = parts[0];
            string name = parts[1];
            string[] parameters = new string[parts.Length - 2];
            Array.Copy(parts, 2, parameters, 0, parameters.Length);

            return (type, name, parameters);
        }
    }
}
