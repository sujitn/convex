using System;
using System.Collections.Concurrent;
using System.Threading;

namespace Convex.Excel
{
    // Response cache for the stateless RPCs: identical (verb, request) is
    // deterministic within one registry generation, so entries live until
    // convex_generation() changes. Error envelopes cache like successes.
    internal static class CxCache
    {
        // Safety valve only; the generation gate is the invalidation path.
        private const int HardCap = 50_000;

        private static readonly ConcurrentDictionary<string, string> _cache = new();
        private static long _generation = -1;
        private static readonly object _genLock = new();

        // Seam for headless tests (no native DLL): defaults to the FFI counter.
        internal static Func<long> GenerationSource = () => unchecked((long)Cx.Generation());

        public static string GetOrCompute(string verb, string requestJson, Func<string> compute)
        {
            long gen;
            try { gen = GenerationSource(); }
            catch { return compute(); } // native lib unavailable — no caching

            if (Volatile.Read(ref _generation) != gen)
            {
                lock (_genLock)
                {
                    if (Volatile.Read(ref _generation) != gen)
                    {
                        _cache.Clear();
                        Volatile.Write(ref _generation, gen);
                    }
                }
            }

            var key = verb + "" + requestJson;
            if (_cache.TryGetValue(key, out var hit)) return hit;

            var raw = compute();

            try
            {
                // Re-validate after the insert too: a mutation between the
                // pre-check and TryAdd would strand a stale entry otherwise.
                if (GenerationSource() == gen)
                {
                    if (_cache.Count >= HardCap) _cache.Clear();
                    _cache.TryAdd(key, raw);
                    if (GenerationSource() != gen) _cache.TryRemove(key, out _);
                }
            }
            catch
            {
                // Generation unreadable — skip caching.
            }
            return raw;
        }

        public static void Clear() => _cache.Clear();
    }
}
