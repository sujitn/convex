using System;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using Convex.Excel;
using Convex.Excel.Ops;
using Xunit;

namespace Convex.Excel.Tests
{
    public class OperationsTests
    {
        [Fact]
        public void The_add_in_carries_the_workspace_version()
        {
            var dir = new DirectoryInfo(AppContext.BaseDirectory);
            while (dir != null && !File.Exists(Path.Combine(dir.FullName, "Cargo.toml"))) dir = dir.Parent;
            Assert.NotNull(dir);
            var cargo = File.ReadAllText(Path.Combine(dir!.FullName, "Cargo.toml"));
            var workspace = Regex.Match(cargo, "(?m)^version\\s*=\\s*\"([^\"]+)\"").Groups[1].Value;

            Assert.Equal(workspace, NativeLoader.AddInVersion);
        }

        [Theory]
        [InlineData("upgrade refused: Authorization: Bearer abcdEFGH1234.xyz-_~+/=", "abcdEFGH1234")]
        [InlineData("token endpoint said {\"access_token\":\"s3cret-access\",\"expires_in\":300}", "s3cret-access")]
        [InlineData("token endpoint said {\"refresh_token\": \"s3cret-refresh\"}", "s3cret-refresh")]
        [InlineData("POST body grant_type=refresh_token&refresh_token=s3cret-refresh&client_id=x", "s3cret-refresh")]
        [InlineData("POST body code_verifier=s3cret-verifier&x=1", "s3cret-verifier")]
        [InlineData("redirect http://127.0.0.1:5000/cb?code=s3cret-code&state=abc", "s3cret-code")]
        [InlineData("raw eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.c2lnbmF0dXJl in the text", "eyJzdWIiOiJ0ZXN0In0")]
        public void Credentials_do_not_survive_cleaning(string text, string secret)
        {
            var clean = LogText.Clean(text);
            Assert.DoesNotContain(secret, clean);
            Assert.Contains("***", clean);
        }

        [Theory]
        [InlineData("{\"code\":\"RESULT_TOO_LARGE\",\"message\":\"too big\"}")]
        [InlineData("the server refused the pasted token: paste a new one")]
        [InlineData("the bearer token expired")]
        [InlineData("session ws-1 open (https://convex.example)")]
        public void Ordinary_text_survives_cleaning(string text) => Assert.Equal(text, LogText.Clean(text));

        [Fact]
        public void A_kept_line_is_one_line_of_bounded_length()
        {
            var clean = LogText.Clean("one\r\ntwo " + new string('x', 5000));
            Assert.DoesNotContain("\n", clean);
            Assert.StartsWith("one  two", clean);
            Assert.True(clean.Length <= 2003);
        }

        [Fact]
        public void The_ring_keeps_the_newest_and_skips_a_repeat_within_a_minute()
        {
            var now = new DateTime(2026, 11, 9, 12, 0, 0, DateTimeKind.Utc);
            var ring = new EventRing(3, () => now);
            Assert.True(ring.Add("a"));
            Assert.False(ring.Add("a"));
            now = now.AddSeconds(61);
            Assert.True(ring.Add("a")); // a minute on, the repeat is news again
            Assert.True(ring.Add("b"));
            Assert.True(ring.Add("c"));
            Assert.True(ring.Add("d"));
            Assert.Equal(new[] { "d", "c", "b" }, ring.Snapshot().Select(e => e.Text));
            ring.Clear();
            Assert.Empty(ring.Snapshot());
            Assert.True(ring.Add("d"));
        }

        [Fact]
        public void Latency_percentiles_are_nearest_rank_over_the_window()
        {
            var window = new LatencyWindow();
            Assert.Null(window.Percentile(0.5));
            for (int i = 1; i <= 100; i++) window.Add(i);
            Assert.Equal(50, window.Percentile(0.5));
            Assert.Equal(99, window.Percentile(0.99));
            for (int i = 0; i < LatencyWindow.Capacity; i++) window.Add(7);
            Assert.Equal(LatencyWindow.Capacity, window.Count);
            Assert.Equal(7, window.Percentile(0.99)); // the old samples are gone
        }

        [Fact]
        public void A_push_outside_a_frame_is_not_timed()
        {
            var timer = new FrameTimer();
            FrameTimer.Pushed();
            timer.Begin();
            timer.End(); // a frame that changed nothing
            Assert.Equal(1, timer.Count);
            Assert.Equal(0, timer.Latency.Count);

            timer.Begin();
            FrameTimer.Pushed();
            timer.End();
            Assert.Equal(2, timer.Count);
            Assert.Equal(1, timer.Latency.Count);

            FrameTimer.Pushed();
            Assert.Equal(1, timer.Latency.Count);
        }
    }

    [CollectionDefinition("log file", DisableParallelization = true)]
    public class LogFileCollection { }

    [Collection("log file")]
    public sealed class LogFileTests : IDisposable
    {
        private readonly string _dir = Path.Combine(Path.GetTempPath(), "convex-log-" + Guid.NewGuid().ToString("N"));

        public void Dispose()
        {
            CxLog.Stop();
            try { Directory.Delete(_dir, true); }
            catch (IOException) { }
        }

        private static string Read(string path)
        {
            using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var reader = new StreamReader(stream, Encoding.UTF8);
            return reader.ReadToEnd();
        }

        private string[] LogFiles() => Directory.GetFiles(_dir).Select(Path.GetFileName).OrderBy(n => n, StringComparer.Ordinal).ToArray()!;

        [Fact]
        public void The_log_rolls_at_its_limit_and_keeps_its_files()
        {
            using (var log = new RollingLog(_dir, "t", maxBytes: 100, files: 3))
            {
                for (int i = 0; i < 40; i++) log.Write($"line {i:00} " + new string('x', 30));
                log.Flush(TimeSpan.FromSeconds(5));
            }
            Assert.Equal(new[] { "t.log", "t.log.1", "t.log.2" }, LogFiles());
            // The newest line is in the current file, and the oldest are gone.
            Assert.Contains("line 39", Read(Path.Combine(_dir, "t.log")));
            Assert.DoesNotContain("line 00", string.Concat(LogFiles().Select(f => Read(Path.Combine(_dir, f)))));
        }

        [Fact]
        public void A_second_excel_writes_its_own_file()
        {
            Directory.CreateDirectory(_dir);
            var held = Path.Combine(_dir, "t.log");
            using (new FileStream(held, FileMode.Append, FileAccess.Write, FileShare.Read))
            using (var log = new RollingLog(_dir, "t"))
            {
                log.Write("hello");
                log.Flush(TimeSpan.FromSeconds(5));
                Assert.NotEqual(held, log.Path);
                Assert.Matches(@"t-\d+\.log$", log.Path);
                Assert.Contains("hello", Read(log.Path));
            }
        }

        [Fact]
        public void A_failure_carrying_a_token_leaves_no_token_on_disk_or_in_the_history()
        {
            const string token = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiJ0cmFkZXIifQ.c2lnbmF0dXJlLWJ5dGVz";
            CxLog.Start(_dir);
            CxLog.Connection("session closed: the upgrade sent Authorization: Bearer " + token);
            CxErrorStore.Record(new InvalidOperationException("request failed with Bearer " + token), "(server)", "server link");
            var path = CxLog.FilePath;
            CxLog.Info("identity provider answered {\"access_token\":\"" + token + "\"}");
            for (int i = 0; i < 200 && CxLog.FilePath.Length == 0; i++) System.Threading.Thread.Sleep(10);
            path = CxLog.FilePath;
            CxLog.Stop();

            var text = Read(path);
            Assert.Contains("session closed", text);
            Assert.Contains("server link fault", text);
            Assert.Contains("identity provider answered", text);
            foreach (var part in token.Split('.')) Assert.DoesNotContain(part, text);
            Assert.DoesNotContain(CxLog.Events.Snapshot(), e => e.Text.Contains("eyJzdWIi"));
        }

        [Fact]
        public void Nothing_is_kept_on_disk_before_the_log_starts()
        {
            CxLog.Connection("no file yet");
            Assert.Equal("", CxLog.FilePath);
            Assert.False(Directory.Exists(_dir));
        }
    }
}
