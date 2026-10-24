using System;
using System.Collections.Concurrent;
using System.Threading;

namespace Convex.Excel
{
    // Response cache for the stateless RPCs: identical (verb, request) is
    // deterministic within one registry generation. Entries are tagged with
    // the generation they were computed under and rejected on mismatch, so a
    // racing mutation can never surface a stale hit; Lazy values give
    // single-flight (N concurrent identical calls share one engine call).
    internal static class CxCache
    {
        // Safety valve only; the generation gate is the invalidation path.
        private const int HardCap = 50_000;

        private readonly struct Entry
        {
            public readonly long Gen;
            public readonly string Raw;
            public Entry(long gen, string raw) { Gen = gen; Raw = raw; }
        }

        private static readonly ConcurrentDictionary<string, Lazy<Entry>> _cache = new();
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

            // Two attempts: the first can lose to a straggler entry from an
            // older generation; evict it and re-add for this generation.
            for (int attempt = 0; attempt < 2; attempt++)
            {
                if (_cache.Count >= HardCap) _cache.Clear();
                var lazy = _cache.GetOrAdd(key,
                    _ => new Lazy<Entry>(() => new Entry(gen, compute())));
                Entry e;
                try { e = lazy.Value; }
                catch
                {
                    _cache.TryRemove(key, out _); // don't cache exceptions
                    throw;
                }
                if (e.Gen == gen) return e.Raw;
                _cache.TryRemove(key, out _);
            }
            return compute();
        }

        public static void Clear() => _cache.Clear();
    }
}
