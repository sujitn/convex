using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Linq;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.WebSockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Newtonsoft.Json.Linq;

// X0 (excel-plan): a loopback server sends `frames` valuation frames shaped like
// convex-server's PublishedValuation (≈30 measures, as after P7a) as fast as the
// socket accepts them. The client receives, parses and keeps the newest frame
// per instrument by revision, as the add-in's ServerSource would. Prints frames/s.
//
//   WsDrain.exe [frames=50000] [instruments=2000] [port=18765]
internal static class Program
{
    private static readonly string[] Measures =
    {
        "clean_price_mid", "dirty_price_mid", "accrued_interest", "yield_to_maturity", "z_spread",
        "i_spread", "asw_spread", "modified_duration", "macaulay_duration", "convexity",
        "effective_duration", "effective_convexity", "spread_duration", "pv01", "cs01", "dv01",
        "krd_3m", "krd_6m", "krd_1y", "krd_2y", "krd_3y", "krd_5y", "krd_7y", "krd_10y", "krd_20y", "krd_30y",
        "g_spread", "discount_margin", "oas", "option_value",
    };

    private static int Main(string[] args)
    {
        int frames = args.Length > 0 ? int.Parse(args[0], CultureInfo.InvariantCulture) : 50_000;
        int instruments = args.Length > 1 ? int.Parse(args[1], CultureInfo.InvariantCulture) : 2_000;
        int port = args.Length > 2 ? int.Parse(args[2], CultureInfo.InvariantCulture) : 18765;

        var listener = new HttpListener();
        listener.Prefixes.Add($"http://localhost:{port}/ws/");
        listener.Start();
        var server = Task.Run(() => Serve(listener, frames, instruments));

        var (received, applied, bytes, seconds) = Drain(port).GetAwaiter().GetResult();
        server.GetAwaiter().GetResult();
        listener.Stop();

        // Parse-only rate of the same frames from memory, to separate the
        // transport from the client's JSON cost.
        var rng = new Random(7);
        var sample = Enumerable.Range(0, 1000).Select(i => Frame(i, i + 1, rng)).ToArray();
        var sw = Stopwatch.StartNew();
        for (int i = 0; i < frames; i++) JObject.Parse(sample[i % sample.Length]);
        double parseSeconds = sw.Elapsed.TotalSeconds;

        string F(double v, string f) => v.ToString(f, CultureInfo.InvariantCulture);
        Console.WriteLine("{\"frames\":" + received + ",\"applied\":" + applied + ",\"instruments\":" + instruments +
            ",\"avg_frame_bytes\":" + F((double)bytes / Math.Max(1, received), "F0") +
            ",\"seconds\":" + F(seconds, "F3") + ",\"frames_per_s\":" + F(received / seconds, "F0") +
            ",\"mb_per_s\":" + F(bytes / seconds / 1e6, "F1") +
            ",\"parse_only_frames_per_s\":" + F(frames / parseSeconds, "F0") + "}");
        return received == frames ? 0 : 1;
    }

    private static async Task Serve(HttpListener listener, int frames, int instruments)
    {
        var ctx = await listener.GetContextAsync();
        var wsCtx = await ctx.AcceptWebSocketAsync(null);
        var ws = wsCtx.WebSocket;
        var rng = new Random(7);
        for (int i = 0; i < frames; i++)
        {
            var bytes = Encoding.UTF8.GetBytes(Frame(i % instruments, i + 1, rng));
            await ws.SendAsync(new ArraySegment<byte>(bytes), WebSocketMessageType.Text, true, CancellationToken.None);
        }
        await ws.CloseOutputAsync(WebSocketCloseStatus.NormalClosure, "done", CancellationToken.None);
    }

    private static async Task<(int, int, long, double)> Drain(int port)
    {
        using var ws = new ClientWebSocket();
        await ws.ConnectAsync(new Uri($"ws://localhost:{port}/ws/"), CancellationToken.None);
        var latest = new Dictionary<string, (long Revision, JObject Frame)>();
        var buffer = new byte[64 * 1024];
        var message = new MemoryStream();
        int received = 0, applied = 0;
        long bytes = 0;
        var sw = Stopwatch.StartNew();
        while (true)
        {
            var r = await ws.ReceiveAsync(new ArraySegment<byte>(buffer), CancellationToken.None);
            if (r.MessageType == WebSocketMessageType.Close) break;
            message.Write(buffer, 0, r.Count);
            if (!r.EndOfMessage) continue;
            bytes += message.Length;
            var frame = JObject.Parse(Encoding.UTF8.GetString(message.GetBuffer(), 0, (int)message.Length));
            message.SetLength(0);
            received++;
            string id = (string)frame["result"]!["provenance"]!["subject"]!["instrument_id"]!;
            long revision = (long)frame["revision"]!;
            if (!latest.TryGetValue(id, out var prev) || revision > prev.Revision)
            {
                latest[id] = (revision, frame);
                applied++;
            }
        }
        return (received, applied, bytes, sw.Elapsed.TotalSeconds);
    }

    private static string Frame(int instrument, long revision, Random rng)
    {
        var sb = new StringBuilder(6 * 1024);
        string id = "US" + instrument.ToString("D9", CultureInfo.InvariantCulture) + "0";
        sb.Append("{\"type\":\"valuation\",\"runtime_id\":\"SHOWCASE-USD-v1\",\"revision\":").Append(revision)
          .Append(",\"generation\":1,\"context_key\":\"ctx-1\",\"live_valid_until\":\"2026-11-05T21:00:05Z\",\"result\":{")
          .Append("\"status\":\"Complete\",\"quality\":\"Valid\",\"snapshot_id\":\"snap-").Append(revision)
          .Append("\",\"configuration_version\":3,\"results\":{");
        for (int m = 0; m < Measures.Length; m++)
        {
            if (m > 0) sb.Append(',');
            sb.Append('"').Append(Measures[m]).Append("\":{\"value\":{\"value\":\"")
              .Append((100 * rng.NextDouble()).ToString("F12", CultureInfo.InvariantCulture))
              .Append("\",\"unit\":\"Percent\"},\"quality\":\"Valid\",\"method\":\"convex-bond-v1\",\"source_observation_id\":\"obs-")
              .Append(revision).Append("\"}");
        }
        sb.Append("},\"errors\":[],\"provenance\":{\"subject\":{\"kind\":\"instrument_version\",\"instrument_id\":\"")
          .Append(id).Append("\",\"version\":1},\"valuation_time\":\"2026-11-05T21:00:00Z\",\"known_at\":\"2026-11-05T21:00:00Z\",")
          .Append("\"calculated_at\":\"2026-11-05T21:00:00.001Z\",\"model_version\":\"convex-0.14.0\"}},")
          .Append("\"last_good\":null,\"retained_quality\":null}");
        return sb.ToString();
    }
}
