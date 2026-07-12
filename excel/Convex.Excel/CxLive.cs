using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using ExcelDna.Integration;
using Newtonsoft.Json.Linq;

namespace Convex.Excel
{
    // RTD-backed live layer: .LIVE cells recompute when the registry
    // generation changes (recompute-all, no dependency graph; fine at
    // hundreds of live cells). Topics share CxCache's (verb, requestJson)
    // key; ExcelAsyncUtil.Observe re-subscribes topics on workbook reopen.
    internal static class CxLive
    {
        public static object Observe(string verb, JObject request, Func<JToken, object> select)
        {
            string requestJson = request.ToString(Newtonsoft.Json.Formatting.None);
            return ExcelAsyncUtil.Observe("CX.LIVE:" + verb, requestJson,
                () => new RpcObservable(verb, requestJson, select));
        }

        private static object Compute(string verb, string requestJson, Func<JToken, object> select)
        {
            try
            {
                return select(Cx.ParseEnvelope(Cx.RawRpc(verb, requestJson)));
            }
            catch (Exception ex)
            {
                CxErrorStore.Record(ex);
                return ErrorMapper.ToExcelError(ex);
            }
        }

        private sealed class RpcObservable : IExcelObservable
        {
            private readonly string _verb;
            private readonly string _requestJson;
            private readonly Func<JToken, object> _select;

            public RpcObservable(string verb, string requestJson, Func<JToken, object> select)
            {
                _verb = verb;
                _requestJson = requestJson;
                _select = select;
            }

            public IDisposable Subscribe(IExcelObserver observer)
            {
                var sub = new Subscription(this, observer);
                sub.Push();
                GenerationWatcher.Add(sub);
                return sub;
            }

            internal sealed class Subscription : IDisposable
            {
                private readonly RpcObservable _owner;
                private readonly IExcelObserver _observer;

                public Subscription(RpcObservable owner, IExcelObserver observer)
                {
                    _owner = owner;
                    _observer = observer;
                }

                public void Push()
                {
                    try
                    {
                        _observer.OnNext(Compute(_owner._verb, _owner._requestJson, _owner._select));
                    }
                    catch (Exception ex)
                    {
                        // OnNext can race RTD topic teardown; an unhandled
                        // throw on a timer thread kills the process.
                        CxErrorStore.Record(ex);
                    }
                }

                public void Dispose() => GenerationWatcher.Remove(this);
            }
        }

        // Polls convex_generation while subscriptions exist. Single-flight:
        // re-armed only after the tick finishes, so ticks never overlap.
        private static class GenerationWatcher
        {
            private static readonly object _lock = new();
            private static readonly HashSet<RpcObservable.Subscription> _subs = new();
            private static Timer? _timer;
            private static long _lastGen;

            private static int PollMs =>
                Math.Max(100, CxSettings.Current.LiveRefreshMs);

            public static void Add(RpcObservable.Subscription sub)
            {
                lock (_lock)
                {
                    _subs.Add(sub);
                    if (_timer == null)
                    {
                        Interlocked.Exchange(ref _lastGen, SafeGeneration());
                        _timer = new Timer(Tick, null, PollMs, Timeout.Infinite);
                    }
                }
            }

            public static void Remove(RpcObservable.Subscription sub)
            {
                lock (_lock)
                {
                    _subs.Remove(sub);
                    if (_subs.Count == 0)
                    {
                        var t = _timer;
                        _timer = null; // Tick's re-arm sees null and stops
                        t?.Dispose();
                    }
                }
            }

            private static void Tick(object? _)
            {
                try
                {
                    long gen = SafeGeneration();
                    if (Interlocked.Exchange(ref _lastGen, gen) != gen)
                    {
                        RpcObservable.Subscription[] subs;
                        lock (_lock) subs = _subs.ToArray();
                        foreach (var s in subs) s.Push();
                    }
                }
                finally
                {
                    lock (_lock) _timer?.Change(PollMs, Timeout.Infinite);
                }
            }

            private static long SafeGeneration()
            {
                try { return unchecked((long)Cx.Generation()); }
                catch { return -1; }
            }
        }
    }
}
