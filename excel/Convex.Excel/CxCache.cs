using System;
using System.Collections.Concurrent;
using System.Threading;

namespace Convex.Excel
{
    // Response cache for the stateless RPCs, keyed (verb + request JSON) and
    // valid for exactly one registry generation: results depend only on the
    // request and the registry state, and convex_generation() bumps on every
    // mutation. Cleared wholesale on generation change; error envelopes are
    // cached like successes (equally deterministic).
    internal static class CxCache
    {
        // Safety valve for pathological sheets (unique mark per cell per
        // recalc); the generation gate is the primary invalidation path.
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
                // Cache only when the registry did not mutate during the
                // compute, and re-validate AFTER the insert: a mutation can
                // land (and another thread can clear the cache) between the
                // pre-insert check and TryAdd, which would otherwise strand
                // this pre-mutation result in the post-mutation cache.
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
