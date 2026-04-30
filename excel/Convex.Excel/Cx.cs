using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
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
        private static extern IntPtr convex_schema([MarshalAs(UnmanagedType.LPUTF8Str)] string typeName);

        [DllImport(Dll)]
        private static extern IntPtr convex_mark_parse([MarshalAs(UnmanagedType.LPUTF8Str)] string text);

        [DllImport(Dll)]
        private static extern IntPtr convex_last_error();

        [DllImport(Dll)]
        private static extern IntPtr convex_version();

        [DllImport(Dll)]
        private static extern void convex_string_free(IntPtr s);

        // ---- Construction --------------------------------------------------

        public static ulong BuildBond(JObject spec)
        {
            ulong h = convex_bond_from_json(spec.ToString(Formatting.None));
            if (h == InvalidHandle)
                throw new ConvexException(LastError() ?? "bond build failed");
            return h;
        }

        public static ulong BuildCurve(JObject spec)
        {
            ulong h = convex_curve_from_json(spec.ToString(Formatting.None));
            if (h == InvalidHandle)
                throw new ConvexException(LastError() ?? "curve build failed");
            return h;
        }

        public static void Release(ulong handle) => convex_release(handle);
        public static int ObjectCount() => convex_object_count();
        public static void ClearAll() => convex_clear_all();

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

        public static JToken Price(JObject request) => Rpc(convex_price, request);
        public static JToken Risk(JObject request) => Rpc(convex_risk, request);
        public static JToken Spread(JObject request) => Rpc(convex_spread, request);
        public static JToken Cashflows(JObject request) => Rpc(convex_cashflows, request);
        public static JToken CurveQuery(JObject request) => Rpc(convex_curve_query, request);
        public static JToken MakeWhole(JObject request) => Rpc(convex_make_whole, request);

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

        private static JToken Rpc(RpcFn fn, JObject request)
        {
            string raw = ConsumeString(fn(request.ToString(Formatting.None)));
            var env = JToken.Parse(raw) ?? throw new ConvexException("empty RPC response");
            if ((string?)env["ok"] != "true")
            {
                var err = env["error"];
                var code = (string?)err?["code"] ?? "error";
                var msg = (string?)err?["message"] ?? "(no message)";
                var field = (string?)err?["field"];
                throw new ConvexException(field == null ? $"{code}: {msg}" : $"{code} ({field}): {msg}");
            }
            return env["result"]!;
        }

        private static string ConsumeString(IntPtr ptr)
        {
            if (ptr == IntPtr.Zero) return string.Empty;
            try { return Utf8Helper.PtrToString(ptr) ?? string.Empty; }
            finally { convex_string_free(ptr); }
        }
    }

    internal sealed class ConvexException : Exception
    {
        public ConvexException(string message) : base(message) { }
    }

    // PtrToStringUTF8 only exists on .NET Core+; net472 needs a manual reader.
    internal static class Utf8Helper
    {
        public static string? PtrToString(IntPtr ptr)
        {
            if (ptr == IntPtr.Zero) return null;
            int len = 0;
            while (Marshal.ReadByte(ptr, len) != 0) len++;
            if (len == 0) return string.Empty;
            var bytes = new byte[len];
            Marshal.Copy(ptr, bytes, 0, len);
            return System.Text.Encoding.UTF8.GetString(bytes);
        }
    }
}
