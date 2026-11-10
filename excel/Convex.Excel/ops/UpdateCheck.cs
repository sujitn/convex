using System;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Convex.Excel.Ops
{
    internal enum UpdateSwitch { On, OffByPolicy, OffByInstaller, OffBySettings }

    /// The update check's settings (DX27), from three places. An administrator's
    /// policy value wins, then the installer's UPDATECHECK=0, then Settings.
    internal sealed class UpdateConfig
    {
        public const string GitHubFeed = "https://github.com/sujitn/convex/releases/latest";
        internal const string PolicyKey = @"Software\Policies\Convex\Excel";
        internal const string InstallerKey = @"Software\Convex\Excel";

        public UpdateConfig(UpdateSwitch @switch, string feed, bool feedByPolicy)
        {
            Switch = @switch;
            Feed = feed;
            FeedByPolicy = feedByPolicy;
        }

        public UpdateSwitch Switch { get; }
        public string Feed { get; }
        public bool FeedByPolicy { get; }
        public bool Enabled => Switch == UpdateSwitch.On;

        /// The registry values are as read: a DWORD, text, or null when missing.
        public static UpdateConfig Resolve(object? policyCheck, object? installerCheck, bool settingsCheck,
                                           object? policyFeed, string? settingsFeed)
        {
            var @switch = IsOff(policyCheck) ? UpdateSwitch.OffByPolicy
                : IsOff(installerCheck) ? UpdateSwitch.OffByInstaller
                : !settingsCheck ? UpdateSwitch.OffBySettings
                : UpdateSwitch.On;
            var byPolicy = (policyFeed as string)?.Trim();
            if (!string.IsNullOrEmpty(byPolicy)) return new UpdateConfig(@switch, byPolicy!, true);
            var bySettings = settingsFeed?.Trim();
            return new UpdateConfig(@switch, string.IsNullOrEmpty(bySettings) ? GitHubFeed : bySettings!, false);
        }

        /// Zero, as a DWORD or as text, turns the check off. Any other value, or none, does not.
        internal static bool IsOff(object? value) => value switch
        {
            int i => i == 0,
            long l => l == 0,
            string s => s.Trim() == "0",
            _ => false,
        };

        public static UpdateConfig Read()
        {
            var settings = CxSettings.Current;
            return Resolve(
                Value(PolicyKey, "UpdateCheck"), Value(InstallerKey, "UpdateCheck"), settings.CheckForUpdates,
                Value(PolicyKey, "UpdateFeed"), settings.UpdateFeed);
        }

        private static object? Value(string key, string name)
        {
            try
            {
                using var opened = Registry.CurrentUser.OpenSubKey(key);
                return opened?.GetValue(name);
            }
            catch { return null; }
        }
    }

    /// What one check found: the latest version and its page, or why there is none.
    internal sealed class UpdateResult
    {
        public UpdateResult(DateTime checkedUtc, string feed, string? latest, string? page, string? failure)
        {
            CheckedUtc = checkedUtc;
            Feed = feed;
            Latest = latest;
            Page = page;
            Failure = failure;
        }

        public DateTime CheckedUtc { get; }
        public string Feed { get; }
        public string? Latest { get; }
        public string? Page { get; }
        public string? Failure { get; }

        internal JObject ToJson() => new()
        {
            ["checked_utc"] = CheckedUtc.ToString("o", CultureInfo.InvariantCulture),
            ["feed"] = Feed,
            ["latest"] = Latest,
            ["page"] = Page,
            ["failure"] = Failure,
        };

        /// Null for anything malformed, which makes the check due.
        internal static UpdateResult? FromJson(JObject o)
        {
            var feed = (string?)o["feed"];
            if (feed == null || !DateTime.TryParse((string?)o["checked_utc"], CultureInfo.InvariantCulture,
                    DateTimeStyles.AdjustToUniversal | DateTimeStyles.AssumeUniversal, out var checkedUtc))
                return null;
            var latest = (string?)o["latest"];
            var page = (string?)o["page"];
            var failure = (string?)o["failure"];
            if (failure == null && (UpdateFeed.ParseVersion(latest) == null || !UpdateFeed.IsWebPage(page))) return null;
            return new UpdateResult(checkedUtc, feed, latest, page, failure);
        }
    }

    /// The check as the ribbon, CX.DIAG and About see it.
    internal sealed class UpdateStatus
    {
        public static readonly UpdateStatus NotStarted = new(null, null, "");

        public UpdateStatus(UpdateConfig? config, UpdateResult? last, string addInVersion)
        {
            Config = config;
            Last = last;
            AddInVersion = addInVersion;
        }

        public UpdateConfig? Config { get; }
        public UpdateResult? Last { get; }
        public string AddInVersion { get; }

        /// The last check, if it read the feed now configured.
        private UpdateResult? Current => Last != null && Config != null && Last.Feed == Config.Feed ? Last : null;

        /// A newer release is known: the ribbon shows its Update button.
        public bool Newer =>
            Config is { Enabled: true } &&
            Current is { Failure: null } found &&
            UpdateFeed.ParseVersion(found.Latest) is { } latest &&
            UpdateFeed.ParseVersion(AddInVersion) is { } running &&
            latest > running;

        public string? Page => Newer ? Current!.Page : null;
        public string? Latest => Newer ? Current!.Latest : null;

        public string Describe()
        {
            switch (Config?.Switch)
            {
                case null: return "not started";
                case UpdateSwitch.OffByPolicy: return "off (policy)";
                case UpdateSwitch.OffByInstaller: return "off (installer)";
                case UpdateSwitch.OffBySettings: return "off (Settings)";
            }
            if (Current is not { } last) return "on, not checked yet";
            var when = " (checked " + last.CheckedUtc.ToString("yyyy-MM-dd HH:mm", CultureInfo.InvariantCulture) + "Z)";
            if (last.Failure != null) return "failed: " + last.Failure + when;
            return Newer ? last.Latest + " available" + when : "up to date: the latest is " + last.Latest + when;
        }
    }

    /// Reads the feed: one GET that should redirect to the latest release's page,
    /// whose last path segment is the version (GitHub's `releases/latest` sends
    /// `.../releases/tag/0.14.0`). The redirect is not followed, so the release
    /// page itself is never fetched, and nothing is ever downloaded.
    internal static class UpdateFeed
    {
        private static readonly Regex Tag = new(@"^v?(\d{1,9})\.(\d{1,9})\.(\d{1,9})$", RegexOptions.CultureInvariant);

        public static Version? ParseVersion(string? text)
        {
            var m = Tag.Match(text ?? "");
            if (!m.Success) return null;
            int Part(int i) => int.Parse(m.Groups[i].Value, CultureInfo.InvariantCulture);
            return new Version(Part(1), Part(2), Part(3));
        }

        public static bool IsWebPage(string? text) =>
            Uri.TryCreate(text, UriKind.Absolute, out var uri) && IsWeb(uri);

        private static bool IsWeb(Uri uri) => uri.Scheme == Uri.UriSchemeHttps || uri.Scheme == Uri.UriSchemeHttp;

        /// The version a release page names in its last path segment.
        public static Version? VersionOf(Uri page)
        {
            var path = Uri.UnescapeDataString(page.AbsolutePath).TrimEnd('/');
            return ParseVersion(path.Substring(path.LastIndexOf('/') + 1));
        }

        /// Through the system proxy, with the user's Windows credentials for a proxy that asks.
        public static HttpClient NewClient() => new(new HttpClientHandler
        {
            AllowAutoRedirect = false,
            DefaultProxyCredentials = CredentialCache.DefaultCredentials,
        })
        { Timeout = TimeSpan.FromSeconds(30) };

        /// Never throws, except when `cancel` is cancelled.
        public static async Task<UpdateResult> ReadAsync(HttpClient http, string feed, string addInVersion,
                                                         DateTime nowUtc, CancellationToken cancel)
        {
            UpdateResult Failed(string why) => new(nowUtc, feed, null, null, why);
            if (!Uri.TryCreate(feed, UriKind.Absolute, out var uri) || !IsWeb(uri))
                return Failed("the feed is not an http or https address");
            try
            {
                using var request = new HttpRequestMessage(HttpMethod.Get, uri);
                request.Headers.UserAgent.ParseAdd("Convex-Excel/" + addInVersion);
                using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancel)
                    .ConfigureAwait(false);
                var code = (int)response.StatusCode;
                if (code >= 300 && code < 400 && response.Headers.Location is { } location)
                {
                    var page = location.IsAbsoluteUri ? location : new Uri(uri, location);
                    if (!IsWeb(page)) return Failed("the feed redirected to a " + page.Scheme + " address, not a web page");
                    var version = VersionOf(page);
                    return version == null
                        ? Failed("the feed redirected to " + page.AbsoluteUri + ", which names no version")
                        : new UpdateResult(nowUtc, feed, version.ToString(3), page.AbsoluteUri, null);
                }
                return Failed(code == 404
                    ? "the feed has no release (HTTP 404)"
                    : "the feed answered HTTP " + code.ToString(CultureInfo.InvariantCulture) + " without a redirect");
            }
            catch (Exception) when (cancel.IsCancellationRequested) { throw; }
            catch (TaskCanceledException) { return Failed("no answer within " + http.Timeout.TotalSeconds.ToString(CultureInfo.InvariantCulture) + " s"); }
            catch (Exception ex) { return Failed(LogText.Clean(Innermost(ex).Message)); }
        }

        private static Exception Innermost(Exception ex)
        {
            while (ex.InnerException != null) ex = ex.InnerException;
            return ex;
        }
    }

    /// The last result, shared by every Excel the user runs, so a restart
    /// does not ask again within the day.
    internal sealed class UpdateCache
    {
        public UpdateCache(string path) => Path = path;

        public string Path { get; }

        public UpdateResult? Read()
        {
            try
            {
                if (!File.Exists(Path)) return null;
                // Dates stay text, so the round trip keeps the instant exactly.
                using var reader = new JsonTextReader(new StringReader(File.ReadAllText(Path))) { DateParseHandling = DateParseHandling.None };
                return JToken.ReadFrom(reader) is JObject o ? UpdateResult.FromJson(o) : null;
            }
            catch { return null; }
        }

        public void Write(UpdateResult result)
        {
            try
            {
                Directory.CreateDirectory(System.IO.Path.GetDirectoryName(Path)!);
                File.WriteAllText(Path, result.ToJson().ToString(Formatting.Indented));
            }
            catch { /* the next start asks again */ }
        }
    }

    /// Asks the feed at most once a day, and only while the check is on.
    internal sealed class UpdateChecker
    {
        public static readonly TimeSpan Interval = TimeSpan.FromDays(1);

        private readonly Func<UpdateConfig> _config;
        private readonly UpdateCache _cache;
        private readonly HttpClient _http;
        private readonly Func<DateTime> _now;
        private readonly string _addInVersion;
        private readonly SemaphoreSlim _gate = new(1, 1);
        private UpdateResult? _last;
        private UpdateStatus _status = UpdateStatus.NotStarted;

        public UpdateChecker(Func<UpdateConfig> config, UpdateCache cache, HttpClient http, Func<DateTime> now, string addInVersion)
        {
            _config = config;
            _cache = cache;
            _http = http;
            _now = now;
            _addInVersion = addInVersion;
        }

        /// Raised when what the ribbon shows may have changed.
        public event Action? Changed;

        public UpdateStatus Status => Volatile.Read(ref _status);

        internal static bool IsDue(UpdateResult? last, string feed, DateTime nowUtc)
        {
            if (last == null || last.Feed != feed) return true;
            var age = nowUtc - last.CheckedUtc;
            // A clock set back must not stop the check for good.
            return age >= Interval || age < TimeSpan.Zero;
        }

        /// Reads the settings and the shared result, then, when `ask` and a day
        /// has passed, the feed. A second call while one runs returns at once.
        public async Task RunAsync(bool ask, CancellationToken cancel)
        {
            if (!await _gate.WaitAsync(0).ConfigureAwait(false)) return;
            try
            {
                var config = _config();
                if (_cache.Read() is { } shared && (_last == null || shared.CheckedUtc > _last.CheckedUtc)) _last = shared;
                if (ask && config.Enabled && IsDue(_last, config.Feed, _now()))
                {
                    var result = await UpdateFeed.ReadAsync(_http, config.Feed, _addInVersion, _now(), cancel).ConfigureAwait(false);
                    _last = result;
                    _cache.Write(result);
                    CxLog.Info("update check of " + result.Feed + ": " +
                               (result.Failure ?? "the latest is " + result.Latest + ", this is " + _addInVersion));
                }
                var before = Status;
                var after = new UpdateStatus(config, _last, _addInVersion);
                Volatile.Write(ref _status, after);
                if (before.Newer != after.Newer || before.Describe() != after.Describe()) Changed?.Invoke();
            }
            finally { _gate.Release(); }
        }
    }

    /// The add-in's one checker: the first look a minute after Excel starts,
    /// then each hour, asking the feed only when a day has passed.
    internal static class UpdateCheck
    {
        private static readonly TimeSpan FirstLook = TimeSpan.FromMinutes(1);
        private static readonly TimeSpan Tick = TimeSpan.FromHours(1);

        private static UpdateChecker? _checker;
        private static Timer? _timer;

        public static event Action? Changed;

        public static UpdateStatus Status => Volatile.Read(ref _checker)?.Status ?? UpdateStatus.NotStarted;

        private static string CachePath => Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Convex", "Excel", "update-check.json");

        public static void Start()
        {
            var checker = new UpdateChecker(UpdateConfig.Read, new UpdateCache(CachePath), UpdateFeed.NewClient(),
                                            () => DateTime.UtcNow, NativeLoader.AddInVersion);
            checker.Changed += () => Changed?.Invoke();
            if (Interlocked.CompareExchange(ref _checker, checker, null) != null) return;
            // Settings, policy and the last result only: the button can show at once.
            Run(ask: false).Wait();
            _timer = new Timer(_ => _ = Run(ask: true), null, FirstLook, Tick);
        }

        public static void Stop()
        {
            Interlocked.Exchange(ref _timer, null)?.Dispose();
        }

        /// Settings were saved: show the change, and check now if it is due.
        public static void SettingsChanged() => Task.Run(() => Run(ask: true));

        private static async Task Run(bool ask)
        {
            try
            {
                if (Volatile.Read(ref _checker) is { } checker) await checker.RunAsync(ask, CancellationToken.None).ConfigureAwait(false);
            }
            catch (Exception ex) { CxLog.Info("update check failed: " + ex.Message); }
        }
    }
}
