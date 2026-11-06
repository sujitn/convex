using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Live
{
    // One local live topic: an FFI verb, its request and the scalar field the
    // cell shows. The topic strings carry the whole spec, so Excel can
    // reconnect a saved workbook's topics with no state from an earlier
    // session, and identical cells share one topic.
    internal sealed class LocalSpec
    {
        public const string Kind = "L1";

        // Excel limits each RTD topic string to 255 characters and a call to
        // 253 strings; three go to the kind, verb and field.
        internal const int ChunkLength = 255;
        internal const int MaxChunks = 250;

        // Request fields the FFI resolves against its registry, per verb.
        // Mirrors PRICE_REFS / RISK_REFS / SPREAD_REFS / YAS_REFS in
        // crates/convex-ffi/src/dispatch.rs, plus the handle-only G-spread
        // benchmark (params.govt_curve).
        private static readonly Dictionary<string, string[][]> RefPaths = new()
        {
            ["price"] = new[] { new[] { "bond" }, new[] { "curve" }, new[] { "forward_curve" } },
            ["risk"] = new[] { new[] { "bond" }, new[] { "curve" }, new[] { "forward_curve" } },
            ["spread"] = new[]
            {
                new[] { "bond" }, new[] { "curve" },
                new[] { "params", "forward_curve" }, new[] { "params", "govt_curve" },
            },
            ["yas"] = new[] { new[] { "bond" }, new[] { "curve" }, new[] { "swap_curve" } },
        };

        public string Verb { get; }
        public string Field { get; }
        public string RequestJson { get; }
        /// Names (aliases or slot names) the request reads, ordinal.
        public IReadOnlyList<string> Names { get; }
        /// Handles the request reads.
        public IReadOnlyList<ulong> Handles { get; }

        private LocalSpec(string verb, string field, string requestJson,
            IReadOnlyList<string> names, IReadOnlyList<ulong> handles)
        {
            Verb = verb;
            Field = field;
            RequestJson = requestJson;
            Names = names;
            Handles = handles;
        }

        public static bool IsLiveVerb(string verb) => RefPaths.ContainsKey(verb);

        public static LocalSpec Create(string verb, string field, JObject request) =>
            FromJson(verb, field, request.ToString(Formatting.None));

        private static LocalSpec FromJson(string verb, string field, string requestJson)
        {
            if (!RefPaths.TryGetValue(verb, out var paths))
                throw new ConvexException($"'{verb}' has no live form");
            var request = JObject.Parse(requestJson);
            var names = new List<string>();
            var handles = new List<ulong>();
            foreach (var path in paths)
            {
                JToken? slot = request;
                foreach (var segment in path) slot = slot?[segment];
                if (slot is not JValue value) continue;
                switch (value.Type)
                {
                    case JTokenType.String:
                        names.Add((string)value!);
                        break;
                    case JTokenType.Integer:
                        handles.Add(Convert.ToUInt64(value.Value, CultureInfo.InvariantCulture));
                        break;
                }
            }
            return new LocalSpec(verb, field, requestJson, names, handles);
        }

        public string[] ToTopicStrings() => TopicStrings(Verb, Field, RequestJson);

        /// Topic strings for XlCall.RTD: kind, verb, field, then the request in
        /// chunks of at most 255 characters. The worksheet path calls this
        /// directly: a live UDF re-runs on every update of its topic, and needs
        /// only the strings (the hub parses the references once, on connect).
        public static string[] TopicStrings(string verb, string field, string requestJson)
        {
            if (!IsLiveVerb(verb))
                throw new ConvexException($"'{verb}' has no live form");
            var strings = new List<string>(4) { Kind, verb, field };
            int start = 0;
            while (start < requestJson.Length)
            {
                int length = Math.Min(ChunkLength, requestJson.Length - start);
                // Never split a surrogate pair across two topic strings.
                if (length > 1 && start + length < requestJson.Length &&
                    char.IsHighSurrogate(requestJson[start + length - 1]))
                    length--;
                strings.Add(start == 0 && length == requestJson.Length
                    ? requestJson
                    : requestJson.Substring(start, length));
                start += length;
                if (strings.Count - 3 > MaxChunks)
                    throw new ConvexException(
                        $"request is {requestJson.Length} characters; a live cell takes at most " +
                        $"about {ChunkLength * MaxChunks}");
            }
            return strings.ToArray();
        }

        public static LocalSpec FromTopicStrings(IList<string> topicInfo)
        {
            if (topicInfo.Count < 4 || topicInfo[0] != Kind)
                throw new ConvexException("not a Convex local live topic (rebuild the cell)");
            var json = new StringBuilder();
            for (int i = 3; i < topicInfo.Count; i++) json.Append(topicInfo[i]);
            return FromJson(topicInfo[1], topicInfo[2], json.ToString());
        }

        /// Key shared by every cell that shows this topic.
        public static string KeyOf(IList<string> topicInfo) => string.Join("\u001f", topicInfo);
    }
}
