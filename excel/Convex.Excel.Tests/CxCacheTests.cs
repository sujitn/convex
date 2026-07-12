using System;
using Convex.Excel;
using Xunit;

namespace Convex.Excel.Tests
{
    [Collection("cache")] // CxCache is global state — serialize these tests
    public class CxCacheTests : IDisposable
    {
        private long _generation = 1;
        private int _computes;

        public CxCacheTests()
        {
            CxCache.Clear();
            CxCache.GenerationSource = () => _generation;
        }

        public void Dispose()
        {
            CxCache.GenerationSource = () => unchecked((long)Cx.Generation());
            CxCache.Clear();
        }

        private string Compute()
        {
            _computes++;
            return $"result-{_computes}";
        }

        [Fact]
        public void Identical_request_hits_cache_within_a_generation()
        {
            var r1 = CxCache.GetOrCompute("price", "{\"bond\":100}", Compute);
            var r2 = CxCache.GetOrCompute("price", "{\"bond\":100}", Compute);
            Assert.Equal(r1, r2);
            Assert.Equal(1, _computes);
        }

        [Fact]
        public void Different_verbs_do_not_collide()
        {
            CxCache.GetOrCompute("price", "{\"bond\":100}", Compute);
            CxCache.GetOrCompute("risk", "{\"bond\":100}", Compute);
            Assert.Equal(2, _computes);
        }

        [Fact]
        public void Generation_bump_invalidates()
        {
            CxCache.GetOrCompute("price", "{\"bond\":100}", Compute);
            _generation++;
            CxCache.GetOrCompute("price", "{\"bond\":100}", Compute);
            Assert.Equal(2, _computes);
        }

        [Fact]
        public void Result_computed_across_a_mutation_is_not_cached()
        {
            // The registry mutates while the compute is in flight: the result
            // must be returned but NOT cached (it may reflect either world).
            var r = CxCache.GetOrCompute("price", "{\"bond\":\"T5\"}", () =>
            {
                _generation++; // mutation mid-compute
                return Compute();
            });
            Assert.Equal("result-1", r);
            // Next call recomputes: nothing stale was kept.
            CxCache.GetOrCompute("price", "{\"bond\":\"T5\"}", Compute);
            Assert.Equal(2, _computes);
        }

        [Fact]
        public void Mutation_racing_the_insert_evicts_the_entry()
        {
            // Interleaving: the pre-insert generation check passes, then a
            // mutation lands while TryAdd runs. The post-add re-validation
            // must evict the entry. Reads per cache miss: gate, pre-insert,
            // post-add — return 1, 1, 2 to model the race, then freeze the
            // source back at 1 so the generation gate can NOT rescue the
            // probe: only the post-add eviction makes it recompute.
            var reads = 0;
            CxCache.GenerationSource = () => ++reads == 3 ? 2 : 1;
            CxCache.GetOrCompute("price", "{\"bond\":\"T7\"}", Compute);
            Assert.Equal(1, _computes);

            CxCache.GenerationSource = () => 1;
            CxCache.GetOrCompute("price", "{\"bond\":\"T7\"}", Compute);
            Assert.Equal(2, _computes);
        }

        [Fact]
        public void Unreadable_generation_bypasses_cache()
        {
            CxCache.GenerationSource = () => throw new DllNotFoundException();
            CxCache.GetOrCompute("price", "{}", Compute);
            CxCache.GetOrCompute("price", "{}", Compute);
            Assert.Equal(2, _computes);
        }
    }
}
