using System.Collections.Generic;
using System.Runtime.InteropServices;
using Convex.Excel.Server;
using ExcelDna.Integration;
using ExcelDna.Integration.Rtd;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Live
{
    /// The one RTD server behind every live cell. Excel-DNA registers it in
    /// process on first use; it only adapts Excel's topic calls to LiveHub.
    /// Excel pulls changed topics once per RTD throttle interval
    /// (Application.RTD.ThrottleInterval, 2,000 ms by default), and only under
    /// Automatic calculation; under Manual the cells update on F9.
    [ComVisible(true)]
    [ProgId(LiveHub.ProgId)]
    public sealed class ConvexRtdServer : ExcelRtdServer
    {
        protected override object ConnectData(Topic topic, IList<string> topicInfo, ref bool newValues) =>
            ToComValue(LiveHub.Instance.Connect(topic.TopicId, topicInfo, new Sink(topic)));

        // Excel-DNA converts an ExcelError passed to Topic.UpdateValue, but
        // returns ConnectData's value to Excel as is, where a boxed ExcelError
        // would show as its number (#REF! as 23). Excel's COM error for
        // C API code c is CVErr(2000 + c), an HRESULT of 0x800A0000 | (2000 + c).
        internal static object ToComValue(object value) =>
            value is ExcelError error
                ? new ErrorWrapper(unchecked((int)0x800A0000) | (2000 + (int)error))
                : value;

        protected override void DisconnectData(Topic topic) =>
            LiveHub.Instance.Disconnect(topic.TopicId);

        private sealed class Sink : ITopicSink
        {
            private readonly Topic _topic;
            public Sink(Topic topic) => _topic = topic;
            public void Push(object value) => _topic.UpdateValue(value);
        }
    }

    /// Worksheet side of a local live cell: validate the field, build the
    /// topic strings and read the topic. The UDF re-runs only when its topic
    /// changes, and then does no engine work: the hub already holds the value.
    internal static class LiveCells
    {
        public static object Local(string verb, string field, JObject request) =>
            Read(LocalSpec.TopicStrings(verb, LiveFields.Validate(verb, field), request.ToString(Formatting.None)));

        /// A CX.LIVE cell. The identifier is checked here, so a malformed one
        /// is #VALUE! at once rather than a topic.
        public static object ServerCell(string identifier, string measure, string field)
        {
            Identifiers.Classify(identifier);
            return Read(ServerCells.TopicStrings(identifier, measure, field));
        }

        /// A CX.STATUS cell.
        public static object StatusCell(string field) => Read(ServerCells.StatusTopicStrings(field));

        /// A CX.LIMIT cell; a malformed book, limit or field is #VALUE! at once.
        public static object LimitCell(string book, string limit, string field) =>
            Read(ServerCells.LimitTopicStrings(book, limit, field));

        private static object Read(string[] strings)
        {
            var value = XlCall.RTD(LiveHub.ProgId, null, strings);
            if (value is ExcelError &&
                LiveHub.Instance.TryGetError(LocalSpec.KeyOf(strings), out var error))
                CxErrorStore.Record(error);
            return value;
        }
    }
}
