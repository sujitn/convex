using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using ExcelDna.Integration;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel
{
    // P/Invoke + envelope-aware RPC. Twelve C symbols cover the entire
    // FFI surface; everything below is a thin wrapper that forwards JSON
    // strings to Rust and unwraps the response envelope.
    internal static class Cx
    {
        private const string Dll = "convex_ffi.dll";

        public const ulong InvalidHandle = 0;

        [DllImport(Dll)]
        private static extern ulong convex_bond_from_json([MarshalAs(UnmanagedType.LPUTF8Str)] string spec);

        [DllImport(Dll)]
        private static extern ulong convex_curve_from_json([MarshalAs(UnmanagedType.LPUTF8Str)] string spec);

        [DllImport(Dll)]
        private static extern IntPtr convex_describe(ulong handle);

        [DllImport(Dll)]
        private static extern void convex_release(ulong handle);

        [DllImport(Dll)]
        private static extern int convex_object_count();

        [DllImport(Dll)]
        private static extern IntPtr convex_list_objects();

        [DllImport(Dll)]
        private static extern void convex_clear_all();

        [DllImport(Dll)]
        private static extern IntPtr convex_price([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_risk([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_spread([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_cashflows([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_curve_query([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_make_whole([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_yas([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_scenario([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_risk_profile([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        // convex_compare has no Excel surface yet (multi-document input —
        // a form job, not a cell one); P/Invoke it when that form exists.
        [DllImport(Dll)]
        private static extern IntPtr convex_hedge([MarshalAs(UnmanagedType.LPUTF8Str)] string req);

        [DllImport(Dll)]
        private static extern IntPtr convex_schema([MarshalAs(UnmanagedType.LPUTF8Str)] string typeName);

        [DllImport(Dll)]
        private static extern IntPtr convex_mark_parse([MarshalAs(UnmanagedType.LPUTF8Str)] string text);

        [DllImport(Dll)]
        private static extern IntPtr convex_last_error();

        [DllImport(Dll)]
        private static extern IntPtr convex_version();

        [DllImport(Dll)]
        private static extern void convex_string_free(IntPtr s);

        [DllImport(Dll)]
        private static extern ulong convex_generation();

        // ---- Construction --------------------------------------------------

        public static ulong BuildBond(JObject spec)
        {
            ApplyCallerKey(spec);
            ulong h = convex_bond_from_json(spec.ToString(Formatting.None));
            if (h == InvalidHandle)
                throw new ConvexException(LastError() ?? "bond build failed");
            return h;
        }

        public static ulong BuildCurve(JObject spec)
        {
            ApplyCallerKey(spec);
            ulong h = convex_curve_from_json(spec.ToString(Formatting.None));
            if (h == InvalidHandle)
                throw new ConvexException(LastError() ?? "curve build failed");
            return h;
        }

        // Scope the registry slot to the calling cell so two cells that
        // reference the same CUSIP / curve name don't evict each other's handle
        // on recalc (Excel's calc order is nondeterministic). No-op outside a
        // cell calculation (ribbon/forms) — there the spec's own id is the key.
        private static void ApplyCallerKey(JObject spec)
        {
            if (spec["registry_key"] != null) return;
            var key = CxCaller.TryGetPrettyKey();
            if (key != null) spec["registry_key"] = key;
        }

        public static void Release(ulong handle) => convex_release(handle);
        public static int ObjectCount() => convex_object_count();
        public static void ClearAll() => convex_clear_all();

        /// Registry mutation counter (stable across idempotent rebuilds).
        public static ulong Generation() => convex_generation();

        public readonly struct ObjectEntry
        {
            public ulong Handle { get; }
            public string Kind { get; }
            public string? Name { get; }
            public ObjectEntry(ulong h, string k, string? n) { Handle = h; Kind = k; Name = n; }
        }

        public static List<ObjectEntry> ListObjects()
        {
            var raw = ConsumeString(convex_list_objects());
            var env = JToken.Parse(raw) ?? throw new ConvexException("empty list response");
            if ((string?)env["ok"] != "true")
                throw new ConvexException((string?)env["error"]?["message"] ?? "list error");
            var arr = env["result"] as JArray ?? new JArray();
            var list = new List<ObjectEntry>(arr.Count);
            foreach (var n in arr)
            {
                if (n is null) continue;
                ulong handle = (ulong)(long)(n["handle"]!);
                string kind = (string?)n["kind"] ?? "?";
                string? name = (string?)n["name"];
                list.Add(new ObjectEntry(handle, kind, name));
            }
            return list;
        }

        // ---- Stateless RPCs ------------------------------------------------

        public static JToken Price(JObject request) => Rpc("price", request);
        public static JToken Risk(JObject request) => Rpc("risk", request);
        public static JToken Spread(JObject request) => Rpc("spread", request);
        public static JToken Cashflows(JObject request) => Rpc("cashflows", request);
        public static JToken CurveQuery(JObject request) => Rpc("curve_query", request);
        public static JToken MakeWhole(JObject request) => Rpc("make_whole", request);
        public static JToken Yas(JObject request) => Rpc("yas", request);
        public static JToken Scenario(JObject request) => Rpc("scenario", request);
        public static JToken RiskProfile(JObject request) => Rpc("risk_profile", request);
        public static JToken Hedge(JObject request) => Rpc("hedge", request);

        // ---- Introspection -------------------------------------------------

        public static string Schema(string typeName)
        {
            var raw = ConsumeString(convex_schema(typeName));
            var env = JToken.Parse(raw) ?? throw new ConvexException("empty schema response");
            if ((string?)env["ok"] != "true")
                throw new ConvexException((string?)env["error"]?["message"] ?? "schema error");
            var result = env["result"];
            return result?.ToString(Formatting.None) ?? "";
        }

        public static string Describe(ulong handle) => ConsumeString(convex_describe(handle));

        public static JToken? ParseMark(string text)
        {
            var raw = ConsumeString(convex_mark_parse(text));
            var env = JToken.Parse(raw);
            if (env == null) return null;
            if ((string?)env["ok"] != "true")
                throw new ConvexException((string?)env["error"]?["message"] ?? "mark parse failed");
            return env["result"];
        }

        public static string Version() => Utf8Helper.PtrToString(convex_version()) ?? "unknown";

        public static string? LastError()
        {
            var ptr = convex_last_error();
            return ptr == IntPtr.Zero ? null : Utf8Helper.PtrToString(ptr);
        }

        // ---- Internals -----------------------------------------------------

        private delegate IntPtr RpcFn(string requestJson);

        private static readonly Dictionary<string, RpcFn> RpcByVerb = new()
        {
            ["price"] = convex_price,
            ["risk"] = convex_risk,
            ["spread"] = convex_spread,
            ["cashflows"] = convex_cashflows,
            ["curve_query"] = convex_curve_query,
            ["make_whole"] = convex_make_whole,
            ["yas"] = convex_yas,
            ["scenario"] = convex_scenario,
            ["risk_profile"] = convex_risk_profile,
            ["hedge"] = convex_hedge,
        };

        /// Raw (cached) envelope for a verb + request JSON — the same key
        /// shape CxLive topics use, so live and static cells share compute.
        internal static string RawRpc(string verb, string requestJson)
        {
            if (!RpcByVerb.TryGetValue(verb, out var fn))
                throw new ConvexException($"unknown RPC verb {verb}");
            return CxCache.GetOrCompute(verb, requestJson, () => ConsumeString(fn(requestJson)));
        }

        /// Result token from an `{"ok":...}` envelope; coded throw on error.
        internal static JToken ParseEnvelope(string raw)
        {
            var env = JToken.Parse(raw) ?? throw new ConvexException("empty RPC response");
            if ((string?)env["ok"] != "true")
            {
                var err = env["error"];
                var code = (string?)err?["code"] ?? ErrorCodes.InvalidInput;
                var msg = (string?)err?["message"] ?? "(no message)";
                var field = (string?)err?["field"];
                throw new ConvexException(field == null ? msg : $"{msg} (field: {field})", code);
            }
            return env["result"]!;
        }

        private static JToken Rpc(string verb, JObject request) =>
            ParseEnvelope(RawRpc(verb, request.ToString(Formatting.None)));

        private static string ConsumeString(IntPtr ptr)
        {
            if (ptr == IntPtr.Zero) return string.Empty;
            try { return Utf8Helper.PtrToString(ptr) ?? string.Empty; }
            finally { convex_string_free(ptr); }
        }
    }

    // `Code` mirrors the FFI envelope codes plus the C#-side "unknown_token";
    // ErrorMapper turns it into the native Excel error a failing UDF returns.
    internal sealed class ConvexException : Exception
    {
        public string Code { get; }

        public ConvexException(string message, string code = ErrorCodes.InvalidInput)
            : base(message) => Code = code;
    }

    internal static class ErrorCodes
    {
        public const string InvalidInput = "invalid_input";
        public const string InvalidHandle = "invalid_handle";
        public const string Analytics = "analytics";
        public const string UnknownToken = "unknown_token";
        public const string Panic = "panic";
    }

    // PtrToStringUTF8 only exists on .NET Core+; net472 needs a manual reader.
    internal static class Utf8Helper
    {
        public static unsafe string? PtrToString(IntPtr ptr)
        {
            if (ptr == IntPtr.Zero) return null;
            byte* p = (byte*)ptr;
            int len = 0;
            while (p[len] != 0) len++;
            return len == 0 ? string.Empty : System.Text.Encoding.UTF8.GetString(p, len);
        }
    }
}
