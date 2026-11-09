using System;
using System.Collections.Generic;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;

namespace Convex.Excel.Server
{
    /// One limit of a book as a CX.LIMIT cell sees it.
    internal sealed class LimitView
    {
        public string BookId { get; set; } = "";
        public string LimitId { get; set; } = "";
        public Subscription Sub { get; set; }
        /// The newest evaluation of the limit, possibly from an earlier session.
        public LimitItem? Item { get; set; }
        /// A status read has answered, so the book's set is known.
        public bool SetKnown { get; set; }
        public ulong SetVersion { get; set; }
        /// The book was read on the open session and is followed on it.
        public bool Fresh { get; set; }
        public string? ProblemCode { get; set; }
        public string? ProblemMessage { get; set; }
        /// Why the last status read failed, while it is retried.
        public string? ReadProblem { get; set; }
        public LinkState Link { get; set; }
        public string? LinkProblem { get; set; }
        public string Server { get; set; } = "";
    }

    /// Books (CX.LIMIT, Excel plan X4) on the session that carries the
    /// instruments.
    ///
    /// - The server streams a limit only when its status changes (DX18), and
    ///   sends nothing on subscribe, so each book is seeded by reading
    ///   `GET .../limits/status` after its subscription is acknowledged: a
    ///   change recorded meanwhile is either in the read or on the stream.
    ///   Per limit the newer evaluation wins (by evaluated_at).
    /// - A change under a newer limit set reads the book again, since a set
    ///   change can drop limits and the stream never says so.
    /// - RESYNC_REQUIRED for the limit stream reads every followed book again.
    /// - A followed book's FORBIDDEN (a stream event, DX19) refuses it until
    ///   Reconnect, a sign-in or a new configuration (as DX14).
    /// - At most 64 books (the server's bound); at most 4 status reads run at
    ///   once; a 429 waits for its Retry-After.
    internal sealed partial class ServerSource
    {
        /// One session follows at most this many books (server D87, D100).
        public const int MaxBooks = 64;
        /// Status reads in flight at once.
        public const int MaxStatusReads = 4;

        private sealed class BookState
        {
            public readonly string Id;
            public readonly long Seq;
            public int Refs;
            public DateTime? LingerUntil;
            public Subscription Sub = Subscription.Wanted;
            public readonly Dictionary<string, LimitItem> Limits = new(StringComparer.Ordinal);
            public bool SetKnown;
            public ulong SetVersion;
            public bool Fresh;
            /// A status read is due.
            public bool ReadWanted;
            public bool Reading;
            /// Bumped when the session or configuration changes, so a read
            /// started before it is dropped.
            public int Epoch;
            public DateTime ReadAt;
            public TimeSpan ReadBackoff;
            public string? ReadProblem;
            public string? ProblemCode;
            public string? ProblemMessage;
            public DateTime? RetryAt;

            public BookState(string id, long seq)
            {
                Id = id;
                Seq = seq;
            }
        }

        private readonly HttpClient _http;
        private readonly Dictionary<string, BookState> _books = new(StringComparer.Ordinal);
        private readonly HashSet<string> _changedBooks = new(StringComparer.Ordinal);
        private int _statusReads;
        private long _bookSeq;

        /// Books whose view changed.
        public event Action<IReadOnlyCollection<string>>? BooksChanged;

        /// A cell follows `bookId`. Never calls back, so a caller may hold its own lock.
        public void AcquireBook(string bookId)
        {
            lock (_gate)
            {
                if (!_books.TryGetValue(bookId, out var book))
                {
                    book = new BookState(bookId, ++_bookSeq);
                    if (ActiveBooks() >= MaxBooks) OverCapBook(book);
                    _books[bookId] = book;
                    _changedBooks.Add(bookId);
                }
                book.Refs++;
                book.LingerUntil = null;
            }
        }

        /// A cell stopped following `bookId`. Never calls back.
        public void ReleaseBook(string bookId)
        {
            lock (_gate)
            {
                if (!_books.TryGetValue(bookId, out var book) || book.Refs == 0) return;
                if (--book.Refs == 0) book.LingerUntil = _now() + _timings.Linger;
            }
        }

        public LimitView LimitView(string bookId, string limitId)
        {
            lock (_gate)
            {
                var view = new LimitView
                {
                    BookId = bookId,
                    LimitId = limitId,
                    Link = _link,
                    LinkProblem = _linkProblem ?? _configProblem ?? _tokens.Problem,
                    Server = _server,
                };
                if (!_books.TryGetValue(bookId, out var book)) return view;
                view.Sub = book.Sub;
                view.Item = book.Limits.TryGetValue(limitId, out var item) ? item : null;
                view.SetKnown = book.SetKnown;
                view.SetVersion = book.SetVersion;
                view.Fresh = book.Fresh && _primary != null && book.Sub == Subscription.Live;
                view.ProblemCode = book.ProblemCode;
                view.ProblemMessage = book.ProblemMessage;
                view.ReadProblem = book.ReadProblem;
                return view;
            }
        }

        // ---------------------------------------------------------------
        // Under the lock: batching, timers and reads.
        // ---------------------------------------------------------------

        private void TickBooks(DateTime now)
        {
            ExpireLingeringBooks(now, unsubscribe: true);
            foreach (var book in _books.Values)
            {
                if (book.Sub == Subscription.Refused && book.RetryAt is { } retry && retry <= now)
                {
                    book.Sub = Subscription.Wanted;
                    ClearProblem(book);
                    _changedBooks.Add(book.Id);
                }
            }
            PromoteOverCapBooks();
            FlushBooks();
            StartStatusReads(now);
        }

        private void ExpireLingeringBooks(DateTime now, bool unsubscribe)
        {
            var gone = _books.Values.Where(b => b.Refs == 0 && b.LingerUntil is { } until && until <= now).ToList();
            if (gone.Count == 0) return;
            var unsub = new List<string>();
            foreach (var book in gone)
            {
                _books.Remove(book.Id);
                book.Epoch++;
                if (book.Sub is Subscription.Sent or Subscription.Live) unsub.Add(book.Id);
            }
            if (unsubscribe && _primary != null && unsub.Count > 0)
            {
                _primary.UnsubscribeLimits(unsub);
                _standby?.UnsubscribeLimits(unsub);
            }
            PromoteOverCapBooks();
        }

        private int ActiveBooks() => _books.Values.Count(b =>
            b.Sub is Subscription.Sent or Subscription.Live || (b.Sub == Subscription.Wanted && b.Refs > 0));

        private static void OverCapBook(BookState book)
        {
            book.Sub = Subscription.OverCap;
            book.ProblemCode = "SESSION_LIMIT";
            book.ProblemMessage = $"one server session follows at most {MaxBooks} books; " +
                                  "remove limit cells of other books to follow this one";
        }

        private void PromoteOverCapBooks()
        {
            int active = ActiveBooks();
            foreach (var book in _books.Values.Where(b => b.Sub == Subscription.OverCap && b.Refs > 0)
                         .OrderBy(b => b.Seq).ToList())
            {
                if (active >= MaxBooks) break;
                book.Sub = Subscription.Wanted;
                ClearProblem(book);
                _changedBooks.Add(book.Id);
                active++;
            }
        }

        /// Send the wanted books, oldest first. A session never lists a book
        /// it already follows (older servers count a repeat against the 64).
        private void FlushBooks()
        {
            if (_primary == null) return;
            var wanted = _books.Values.Where(b => b.Sub == Subscription.Wanted && b.Refs > 0)
                .OrderBy(b => b.Seq).Select(b => b.Id).Take(MaxBooks).ToList();
            if (wanted.Count == 0) return;
            _primary.SubscribeLimits(wanted);
            _standby?.SubscribeLimits(wanted);
            foreach (var id in wanted) _books[id].Sub = Subscription.Sent;
        }

        /// A standby session takes every followed book before it takes over.
        private int SubscribeFollowedBooks(ServerSession session)
        {
            var followed = _books.Values.Where(b => b.Sub is Subscription.Sent or Subscription.Live)
                .OrderBy(b => b.Seq).Select(b => b.Id).ToList();
            if (followed.Count > 0) session.SubscribeLimits(followed);
            return followed.Count;
        }

        /// A new server or sign-in: every book is read afresh.
        private void ConfigureBooks()
        {
            foreach (var book in _books.Values)
            {
                book.Epoch++;
                book.Limits.Clear();
                book.SetKnown = book.Fresh = book.ReadWanted = false;
                book.SetVersion = 0;
                book.ReadProblem = null;
                book.ReadBackoff = TimeSpan.Zero;
                if (book.Sub is Subscription.Sent or Subscription.Live or Subscription.Refused)
                {
                    book.Sub = Subscription.Wanted;
                    ClearProblem(book);
                }
                _changedBooks.Add(book.Id);
            }
        }

        private void RetryRefusedBooks(bool all)
        {
            foreach (var book in _books.Values.Where(b => b.Sub == Subscription.Refused))
            {
                if (!all && book.RetryAt == null) continue;
                book.Sub = Subscription.Wanted;
                ClearProblem(book);
                _changedBooks.Add(book.Id);
            }
        }

        /// Read every followed book again (RESYNC_REQUIRED for limits).
        private void ResyncBooks()
        {
            foreach (var book in _books.Values.Where(b => b.Sub == Subscription.Live))
            {
                book.ReadWanted = true;
                book.ReadAt = _now();
            }
        }

        /// The open session is gone: limits stay, marked Stale (DX6), and
        /// each book is read again once the next session follows it.
        private void LoseBookSession()
        {
            foreach (var book in _books.Values)
            {
                book.Epoch++;
                book.Fresh = book.ReadWanted = false;
                if (book.Sub is Subscription.Sent or Subscription.Live) book.Sub = Subscription.Wanted;
                _changedBooks.Add(book.Id);
            }
        }

        private void StartStatusReads(DateTime now)
        {
            if (_primary == null || _httpBase == null || !_tokens.Ready) return;
            foreach (var book in _books.Values.Where(b => b.ReadWanted && !b.Reading && b.ReadAt <= now &&
                                                          b.Sub == Subscription.Live)
                         .OrderBy(b => b.Seq).ToList())
            {
                if (_statusReads >= MaxStatusReads) return;
                book.ReadWanted = false;
                book.Reading = true;
                _statusReads++;
                var epoch = book.Epoch;
                var baseUri = _httpBase;
                var token = _tokens.Token;
                _ = Task.Run(() => ReadStatusAsync(book, epoch, baseUri, token));
            }
        }

        private async Task ReadStatusAsync(BookState book, int epoch, Uri baseUri, string? token)
        {
            LimitAnswer answer;
            try
            {
                answer = await LimitRoutes.StatusAsync(_http, baseUri, token, book.Id, CancellationToken.None)
                    .ConfigureAwait(false);
            }
            catch (Exception ex)
            {
                answer = new LimitAnswer { Problem = "the status read failed: " + ex.Message };
            }
            bool rejected = false;
            lock (_gate)
            {
                _statusReads--;
                book.Reading = false;
                if (epoch == book.Epoch && _books.TryGetValue(book.Id, out var current) && current == book)
                    rejected = StatusRead(book, answer);
            }
            if (rejected) _tokens.Rejected();
            Notify();
        }

        /// Apply a status read; true when the server refused the sign-in.
        private bool StatusRead(BookState book, LimitAnswer answer)
        {
            var now = _now();
            _changedBooks.Add(book.Id);
            if (answer.Read is { } read)
            {
                // Per limit the newer evaluation wins: a change on the stream
                // can be newer than this read. A limit the set no longer
                // holds goes, unless a change under a newer set names it.
                var merged = new Dictionary<string, LimitItem>(StringComparer.Ordinal);
                foreach (var item in read.Limits) merged[item.LimitId] = item;
                foreach (var kept in book.Limits.Values)
                {
                    if (kept.LimitSetVersion < read.LimitSetVersion || kept.EvaluatedAtUtc <= read.EvaluatedAtUtc) continue;
                    if (kept.LimitSetVersion > read.LimitSetVersion || merged.ContainsKey(kept.LimitId))
                        merged[kept.LimitId] = kept;
                }
                book.Limits.Clear();
                foreach (var item in merged.Values) book.Limits[item.LimitId] = item;
                book.SetKnown = true;
                book.SetVersion = read.LimitSetVersion;
                book.Fresh = true;
                book.ReadProblem = null;
                book.ReadBackoff = TimeSpan.Zero;
                if (book.ProblemCode == "NO_LIMIT_SET") ClearProblem(book);
                return false;
            }
            switch ((int?)answer.Status)
            {
                case 404:
                    // No set (or no book): stay subscribed, since committing a
                    // set publishes each limit's first status.
                    book.Limits.Clear();
                    book.SetKnown = false;
                    book.Fresh = true;
                    book.ReadProblem = null;
                    book.ProblemCode = "NO_LIMIT_SET";
                    book.ProblemMessage = answer.Problem;
                    return false;
                case 403:
                    Refuse(book, "FORBIDDEN", answer.Problem, retry: false, unsubscribe: true);
                    return false;
                case 400:
                    Refuse(book, answer.Code ?? "INVALID_INPUT", $"the server refused book {book.Id}: {answer.Problem}",
                        retry: false, unsubscribe: true);
                    return false;
            }
            // 401, 429, 5xx or no answer: keep what is shown and read again.
            if (book.ReadProblem != answer.Problem) Ops.CxLog.Connection($"book {book.Id} status read failed: {answer.Problem}");
            book.ReadProblem = answer.Problem;
            book.ReadWanted = true;
            if (answer.Status == (HttpStatusCode)429)
            {
                book.ReadAt = now + (answer.RetryAfter ?? _timings.TooManyRequests);
            }
            else
            {
                book.ReadBackoff = book.ReadBackoff == TimeSpan.Zero
                    ? _timings.StatusRetry
                    : TimeSpan.FromTicks(Math.Min(_timings.BackoffMax.Ticks, book.ReadBackoff.Ticks * 2));
                book.ReadAt = now + Jitter(book.ReadBackoff);
            }
            return answer.Status == HttpStatusCode.Unauthorized;
        }

        /// Stop following a refused book until Reconnect, a sign-in or a new
        /// configuration (or after a minute, for `retry`). `unsubscribe` for a
        /// book the session follows; a refused subscription follows nothing.
        private void Refuse(BookState book, string code, string message, bool retry, bool unsubscribe)
        {
            bool followed = unsubscribe && book.Sub is Subscription.Sent or Subscription.Live;
            // A quota refusal is retried each minute; say so once.
            if (book.ProblemCode != code) Ops.CxLog.Connection($"book {book.Id} refused: {code} {message}");
            book.Sub = Subscription.Refused;
            book.ProblemCode = code;
            book.ProblemMessage = message;
            book.RetryAt = retry ? _now() + _timings.RetryRefused : null;
            book.Limits.Clear();
            book.SetKnown = book.Fresh = book.ReadWanted = false;
            book.Epoch++;
            _changedBooks.Add(book.Id);
            if (followed && _primary != null)
            {
                _primary.UnsubscribeLimits(new[] { book.Id });
                _standby?.UnsubscribeLimits(new[] { book.Id });
            }
        }

        private static void ClearProblem(BookState book)
        {
            book.ProblemCode = null;
            book.ProblemMessage = null;
            book.RetryAt = null;
        }

        // ---------------------------------------------------------------
        // Session events (receive threads).
        // ---------------------------------------------------------------

        public void LimitsSubscribed(ServerSession session, string[] bookIds)
        {
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                _lastMessage = _now();
                foreach (var id in bookIds)
                {
                    if (!_books.TryGetValue(id, out var book) || book.Sub != Subscription.Sent) continue;
                    // Subscribed first, then read: nothing recorded in between is missed.
                    book.Sub = Subscription.Live;
                    book.ReadWanted = true;
                    book.ReadAt = _now();
                    _changedBooks.Add(id);
                }
                if (session == _standby && !session.IsClosed && session.PendingSubscribes == 0) Promote();
                StartStatusReads(_now());
            }
            Notify();
        }

        public void LimitsRefused(ServerSession session, string[] bookIds, string code, string message)
        {
            bool rejected = false;
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                if (code == "UNAUTHENTICATED")
                {
                    rejected = true;
                    session.Abort("the server no longer accepts the sign-in");
                }
                else if (message == LimitErrors.NotMonitored)
                {
                    foreach (var id in bookIds)
                        if (_books.TryGetValue(id, out var book) && book.Sub == Subscription.Sent)
                            Refuse(book, "NO_MONITOR",
                                "the server does not monitor limits, so CX.LIMIT cannot follow them; " +
                                "the pre-trade ticket still reads a book's current status", retry: false,
                                unsubscribe: false);
                }
                else if (bookIds.Length > 1)
                {
                    // A refusal names no book: split until each refused one stands alone (as DX14).
                    int half = bookIds.Length / 2;
                    session.SubscribeLimits(bookIds.Take(half).ToArray());
                    session.SubscribeLimits(bookIds.Skip(half).ToArray());
                }
                else if (bookIds.Length == 1 && _books.TryGetValue(bookIds[0], out var book))
                {
                    var id = bookIds[0];
                    Refuse(book, code, code switch
                    {
                        "FORBIDDEN" => $"you are not entitled to the limits of book {id} (FORBIDDEN)",
                        "ACCESS_QUOTA_EXCEEDED" => $"the server's subscription quota for your sign-in is used up; book {id} is retried in a minute",
                        "ACCESS_UNAVAILABLE" => $"the server could not check access to book {id}; it is retried in a minute",
                        _ => $"the server refused book {id}: {code}" + (message.Length > 0 && message != code ? " (" + message + ")" : ""),
                    }, retry: code is "ACCESS_QUOTA_EXCEEDED" or "ACCESS_UNAVAILABLE", unsubscribe: false);
                }
                if (session == _standby && !session.IsClosed && session.PendingSubscribes == 0) Promote();
            }
            if (rejected) _tokens.Rejected();
            Notify();
        }

        public void LimitStatus(ServerSession session, string bookId, LimitItem item)
        {
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                _lastMessage = _now();
                _infoChanged = true;
                if (!_books.TryGetValue(bookId, out var book) || book.Sub is Subscription.Refused or Subscription.OverCap)
                    return;
                if (item.LimitSetVersion > book.SetVersion && (book.SetKnown || book.ProblemCode == "NO_LIMIT_SET"))
                {
                    // A newer set may have dropped limits; only a read says which.
                    book.ReadWanted = true;
                    book.ReadAt = _now();
                }
                if (book.Limits.TryGetValue(item.LimitId, out var kept) && kept.EvaluatedAtUtc > item.EvaluatedAtUtc)
                    return;
                book.Limits[item.LimitId] = item;
                if (book.ProblemCode == "NO_LIMIT_SET") ClearProblem(book);
                _changedBooks.Add(bookId);
                StartStatusReads(_now());
            }
            Notify();
        }

        public void LimitForbidden(ServerSession session, string bookId)
        {
            lock (_gate)
            {
                if (session != _primary && session != _standby) return;
                _lastMessage = _now();
                if (!_books.TryGetValue(bookId, out var book) || book.Sub == Subscription.Refused) return;
                Refuse(book, "FORBIDDEN",
                    $"you are not entitled to the limits of book {bookId}: they need read access to each " +
                    "instrument it holds and each one a limit names (FORBIDDEN)", retry: false, unsubscribe: true);
            }
            Notify();
        }
    }
}
