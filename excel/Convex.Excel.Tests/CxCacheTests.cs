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
        public void Concurrent_identical_requests_share_one_compute()
        {
            // Single-flight: the second caller must block on the first's
            // in-flight Lazy instead of invoking the engine again.
            var started = new System.Threading.ManualResetEventSlim();
            var release = new System.Threading.ManualResetEventSlim();
            int computes = 0;
            string SlowCompute()
            {
                System.Threading.Interlocked.Increment(ref computes);
                started.Set();
                release.Wait(5000);
                return "r";
            }
            var t1 = System.Threading.Tasks.Task.Run(
                () => CxCache.GetOrCompute("price", "{\"bond\":100}", SlowCompute));
            started.Wait(5000);
            var t2 = System.Threading.Tasks.Task.Run(
                () => CxCache.GetOrCompute("price", "{\"bond\":100}", SlowCompute));
            release.Set();
            Assert.Equal("r", t1.Result);
            Assert.Equal("r", t2.Result);
            Assert.Equal(1, computes);
        }

        [Fact]
        public void Entry_from_an_older_generation_is_rejected_on_hit()
        {
            // A straggler computed under gen 1 must not be served to a gen-2
            // reader even if the wholesale clear has not run for it: entries
            // are tagged and validated on every hit.
            CxCache.GetOrCompute("price", "{\"bond\":\"T7\"}", Compute); // tag: gen 1
            _generation = 2;
            CxCache.GetOrCompute("price", "{\"bond\":\"T7\"}", Compute);
            Assert.Equal(2, _computes);
        }

        [Fact]
        public void Compute_exceptions_are_not_cached()
        {
            Assert.Throws<InvalidOperationException>(() =>
                CxCache.GetOrCompute("price", "{}", () => throw new InvalidOperationException()));
            CxCache.GetOrCompute("price", "{}", Compute);
            Assert.Equal(1, _computes);
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
