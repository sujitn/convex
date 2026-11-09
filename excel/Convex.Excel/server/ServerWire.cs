using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Server
{
    /// JSON reading for server messages: dates and decimals stay strings, so a
    /// timestamp or an exact decimal reaches the parser exactly as sent.
    internal static class ServerJson
    {
        public static JObject Parse(string text)
        {
            using var reader = new JsonTextReader(new StringReader(text))
            {
                DateParseHandling = DateParseHandling.None,
                FloatParseHandling = FloatParseHandling.Double,
                MaxDepth = 64,
            };
            return JObject.Load(reader);
        }

        /// A server UTC timestamp (YYYY-MM-DDTHH:MM:SS[.fraction]Z, up to nine
        /// fractional digits) as a UTC DateTime; .NET keeps seven digits.
        public static DateTime? Timestamp(JToken? token)
        {
            if (token is not JValue { Type: JTokenType.String } v) return null;
            var s = (string)v!;
            if (s.Length < 20 || s[s.Length - 1] != 'Z') return null;
            var body = s.Substring(0, s.Length - 1);
            int dot = body.IndexOf('.');
            if (dot >= 0 && body.Length - dot - 1 > 7) body = body.Substring(0, dot + 8);
            return DateTime.TryParseExact(body, "yyyy-MM-ddTHH:mm:ss.FFFFFFF", CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeUniversal | DateTimeStyles.AdjustToUniversal, out var t)
                ? t
                : null;
        }

        /// A number sent as a JSON number or as an exact decimal string.
        public static double? Number(JToken? token) => token switch
        {
            JValue { Type: JTokenType.Float or JTokenType.Integer } v => Convert.ToDouble(v.Value, CultureInfo.InvariantCulture),
            JValue { Type: JTokenType.String } v when double.TryParse((string)v!, NumberStyles.Float,
                CultureInfo.InvariantCulture, out var d) => d,
            _ => null,
        };

        public static string? Text(JToken? token) =>
            token is JValue { Type: JTokenType.String } v ? (string)v! : null;
    }

    /// One measure of a result: its scalar (when it has one) and its quality.
    internal sealed class Measure
    {
        public double? Value { get; }
        public string Quality { get; }
        public string? Unit { get; }

        public Measure(double? value, string quality, string? unit)
        {
            Value = value;
            Quality = quality;
            Unit = unit;
        }
    }

    /// One error a result carries.
    internal sealed class ResultError
    {
        public string Code { get; }
        public string? Field { get; }
        public string Message { get; }

        public ResultError(string code, string? field, string message)
        {
            Code = code;
            Field = field;
            Message = message;
        }
    }

    /// The parts of a server ResultEnvelope a cell can show.
    internal sealed class Envelope
    {
        public string Status { get; }
        public string Quality { get; }
        public string? SnapshotId { get; }
        public double? ConfigurationVersion { get; }
        public DateTime? CalculatedAtUtc { get; }
        public IReadOnlyDictionary<string, Measure> Results { get; }
        public IReadOnlyList<ResultError> Errors { get; }

        private Envelope(string status, string quality, string? snapshotId, double? configurationVersion,
            DateTime? calculatedAtUtc, IReadOnlyDictionary<string, Measure> results, IReadOnlyList<ResultError> errors)
        {
            Status = status;
            Quality = quality;
            SnapshotId = snapshotId;
            ConfigurationVersion = configurationVersion;
            CalculatedAtUtc = calculatedAtUtc;
            Results = results;
            Errors = errors;
        }

        public static Envelope? Parse(JToken? token)
        {
            if (token is not JObject o) return null;
            var results = new Dictionary<string, Measure>(StringComparer.Ordinal);
            if (o["results"] is JObject measures)
            {
                foreach (var p in measures.Properties())
                {
                    if (p.Value is not JObject m) continue;
                    var value = m["value"] as JObject;
                    results[p.Name] = new Measure(
                        ServerJson.Number(value?["value"]),
                        ServerJson.Text(m["quality"]) ?? "",
                        ServerJson.Text(value?["unit"]));
                }
            }
            var errors = (o["errors"] as JArray ?? new JArray()).OfType<JObject>()
                .Select(e => new ResultError(ServerJson.Text(e["code"]) ?? "", ServerJson.Text(e["field"]),
                    ServerJson.Text(e["message"]) ?? ""))
                .ToList();
            return new Envelope(
                ServerJson.Text(o["status"]) ?? "",
                ServerJson.Text(o["quality"]) ?? "",
                ServerJson.Text(o["snapshot_id"]),
                ServerJson.Number(o["configuration_version"]),
                ServerJson.Timestamp(o["provenance"]?["calculated_at"]),
                results,
                errors);
        }
    }

    /// A `valuation` message: one instrument's published result.
    internal sealed class Valuation
    {
        public string RuntimeId { get; }
        public ulong Revision { get; }
        public string InstrumentId { get; }
        public Envelope Result { get; }
        public Envelope? LastGood { get; }
        public string? RetainedQuality { get; }

        public Valuation(string runtimeId, ulong revision, string instrumentId, Envelope result,
            Envelope? lastGood, string? retainedQuality)
        {
            RuntimeId = runtimeId;
            Revision = revision;
            InstrumentId = instrumentId;
            Result = result;
            LastGood = lastGood;
            RetainedQuality = retainedQuality;
        }

        /// Null when the message is not an instrument's valuation.
        public static Valuation? Parse(JObject message)
        {
            var subject = message["result"]?["provenance"]?["subject"];
            var instrument = ServerJson.Text(subject?["instrument_id"]);
            var runtime = ServerJson.Text(message["runtime_id"]);
            var result = Envelope.Parse(message["result"]);
            if (instrument == null || runtime == null || result == null ||
                message["revision"] is not JValue { Type: JTokenType.Integer } revision)
                return null;
            return new Valuation(runtime, Convert.ToUInt64(revision.Value, CultureInfo.InvariantCulture),
                instrument, result, Envelope.Parse(message["last_good"]),
                ServerJson.Text(message["retained_quality"]));
        }

        /// Whether this frame replaces `current` for its instrument: a frame
        /// from another runtime does, and within a runtime a later revision does.
        public bool Supersedes(string? runtimeId, ulong revision) =>
            runtimeId == null || runtimeId != RuntimeId || Revision > revision;
    }

    /// The instrument a RESULT_TOO_LARGE error names. The server's message
    /// starts "The result of instrument {id} (runtime {runtime}, revision {n})";
    /// an instrument id has no spaces or parentheses, so the parse is exact.
    internal static class TooLarge
    {
        private static readonly Regex Pattern = new(
            @"^The result of instrument (?<id>\S+) \(runtime (?<runtime>.+?), revision (?<revision>\d+)\)",
            RegexOptions.CultureInvariant);

        public static bool TryParse(string message, out string instrumentId, out string runtimeId, out ulong revision)
        {
            var m = Pattern.Match(message);
            instrumentId = m.Groups["id"].Value;
            runtimeId = m.Groups["runtime"].Value;
            revision = 0;
            return m.Success && ulong.TryParse(m.Groups["revision"].Value, NumberStyles.None,
                CultureInfo.InvariantCulture, out revision);
        }
    }

    internal enum IdentifierKind { Instrument, Cusip, Isin }

    /// What a cell's identifier names. A CUSIP or ISIN with a valid check
    /// digit is resolved through the server's reference master; anything else
    /// is the server's instrument id.
    internal static class Identifiers
    {
        public static (IdentifierKind Kind, string Value) Classify(string raw)
        {
            var s = raw.Trim();
            var upper = s.ToUpperInvariant();
            if (IsCusip(upper)) return (IdentifierKind.Cusip, upper);
            if (IsIsin(upper)) return (IdentifierKind.Isin, upper);
            if (IsInstrumentId(s)) return (IdentifierKind.Instrument, s);
            throw new ConvexException(
                $"'{raw}' is not a CUSIP, an ISIN or a server instrument id " +
                "(1 to 128 letters, digits or _ . : / -, starting with a letter or digit)");
        }

        /// The server's InstrumentId grammar: 1–128 ASCII characters, the
        /// first a letter or digit, the rest letters, digits or `_.:/-`.
        public static bool IsInstrumentId(string s) =>
            s.Length is >= 1 and <= 128 && IsAsciiAlnum(s[0]) &&
            s.All(c => IsAsciiAlnum(c) || c is '_' or '.' or ':' or '/' or '-');

        public static bool IsCusip(string s)
        {
            if (s.Length != 9 || !char.IsDigit(s[8])) return false;
            int sum = 0;
            for (int i = 0; i < 8; i++)
            {
                char c = s[i];
                int v;
                if (c is >= '0' and <= '9') v = c - '0';
                else if (c is >= 'A' and <= 'Z') v = c - 'A' + 10;
                else if (c == '*') v = 36;
                else if (c == '@') v = 37;
                else if (c == '#') v = 38;
                else return false;
                if (i % 2 == 1) v *= 2;
                sum += v / 10 + v % 10;
            }
            return (10 - sum % 10) % 10 == s[8] - '0';
        }

        public static bool IsIsin(string s)
        {
            if (s.Length != 12 || !char.IsLetter(s[0]) || !char.IsLetter(s[1]) || !char.IsDigit(s[11]))
                return false;
            var digits = new System.Text.StringBuilder(24);
            foreach (char c in s)
            {
                if (c is >= '0' and <= '9') digits.Append(c);
                else if (c is >= 'A' and <= 'Z') digits.Append((c - 'A' + 10).ToString(CultureInfo.InvariantCulture));
                else return false;
            }
            // Luhn over the expanded digits, check digit included.
            int sum = 0;
            bool twice = false;
            for (int i = digits.Length - 1; i >= 0; i--)
            {
                int d = digits[i] - '0';
                if (twice)
                {
                    d *= 2;
                    if (d > 9) d -= 9;
                }
                sum += d;
                twice = !twice;
            }
            return sum % 10 == 0;
        }

        private static bool IsAsciiAlnum(char c) => c is >= '0' and <= '9' or >= 'a' and <= 'z' or >= 'A' and <= 'Z';
    }

    internal enum SignInMode { Oidc, Token, None }

    /// The per-user server link settings, in %APPDATA%\Convex\server.json.
    /// Tokens are never written to it.
    internal sealed class ServerConfig
    {
        public string Url { get; set; } = "";
        public SignInMode SignIn { get; set; } = SignInMode.Oidc;
        public string Issuer { get; set; } = "";
        public string ClientId { get; set; } = "";
        public string Scope { get; set; } = "openid offline_access";
        public string? Resource { get; set; }
        public string RedirectHost { get; set; } = "127.0.0.1";

        public bool Configured => Url.Length > 0;

        private static string FilePath =>
            Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Convex", "server.json");

        public static ServerConfig Load()
        {
            try
            {
                if (!File.Exists(FilePath)) return new ServerConfig();
                return FromJson(ServerJson.Parse(File.ReadAllText(FilePath)));
            }
            catch
            {
                return new ServerConfig();
            }
        }

        public void Save()
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            File.WriteAllText(FilePath, ToJson().ToString(Formatting.Indented));
        }

        internal static ServerConfig FromJson(JObject o) => new()
        {
            Url = ServerJson.Text(o["url"]) ?? "",
            SignIn = (ServerJson.Text(o["sign_in"]) ?? "oidc").ToLowerInvariant() switch
            {
                "token" => SignInMode.Token,
                "none" => SignInMode.None,
                _ => SignInMode.Oidc,
            },
            Issuer = ServerJson.Text(o["issuer"]) ?? "",
            ClientId = ServerJson.Text(o["client_id"]) ?? "",
            Scope = ServerJson.Text(o["scope"]) ?? "openid offline_access",
            Resource = ServerJson.Text(o["resource"]),
            RedirectHost = ServerJson.Text(o["redirect_host"]) ?? "127.0.0.1",
        };

        internal JObject ToJson() => new()
        {
            ["url"] = Url,
            ["sign_in"] = SignIn.ToString().ToLowerInvariant(),
            ["issuer"] = Issuer,
            ["client_id"] = ClientId,
            ["scope"] = Scope,
            ["resource"] = Resource,
            ["redirect_host"] = RedirectHost,
        };

        /// The server's HTTP base, or a ConvexException saying what is wrong.
        /// Plain http is accepted only for a loopback host, so a token never
        /// crosses a network in clear.
        public Uri HttpBase()
        {
            if (!Uri.TryCreate(Url.Trim(), UriKind.Absolute, out var uri) ||
                (uri.Scheme != Uri.UriSchemeHttps && uri.Scheme != Uri.UriSchemeHttp) ||
                (uri.AbsolutePath != "/" && uri.AbsolutePath.Length > 0) || uri.Query.Length > 0)
                throw new ConvexException(
                    $"server URL '{Url}' must be https://host[:port] (or http:// for this machine)");
            if (uri.Scheme == Uri.UriSchemeHttp && !uri.IsLoopback)
                throw new ConvexException(
                    $"server URL '{Url}' uses http to another machine; use https so the sign-in token is encrypted");
            return new Uri(uri.GetLeftPart(UriPartial.Authority) + "/");
        }

        /// The WebSocket endpoint beside HttpBase.
        public Uri WebSocketUri()
        {
            var http = HttpBase();
            var ws = new UriBuilder(http) { Scheme = http.Scheme == Uri.UriSchemeHttps ? "wss" : "ws", Path = "/api/v1/ws" };
            return ws.Uri;
        }
    }
}
