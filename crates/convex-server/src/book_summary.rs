//! Book summaries: the `book` topic.
//!
//! The limit monitor evaluates each watched book (one with a non-empty limit set, or in
//! force with a close) after a tick revalues an instrument it holds, and from that one
//! evaluation builds the book's summary: its risk (each measure the value a limit on it
//! reports), its P&L since its latest close as a total, and each limit's status, value and
//! utilisation of the moment, as the status route reads them. Reasons and excesses stay on
//! the status route and the limit stream, which keeps a summary of 64 limits small enough
//! for one message.
//!
//! A book's summaries go out at most once every [`SUMMARY_INTERVAL`]: one evaluated
//! within the interval after the last sent waits for its end, and a later one replaces
//! it, so the last of a burst is always sent. Each watched book's latest summary is kept
//! for a session's subscription to start from. Of two summaries from one monitor, the one
//! with the higher `sequence` is the later.
#![forbid(unsafe_code)]
#![deny(
    unused_must_use,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use convex_core::contracts::{BusinessDate, ExactDecimal, UtcTimestamp, Version};
use convex_engine::ports::limits::{BookRisk, LimitEvaluation, LimitId, LimitStatus};
use convex_engine::ports::reference_master::BookId;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::book_close::NoBookPnlReason;

/// The shortest time between two summaries of one book.
pub(crate) const SUMMARY_INTERVAL: Duration = Duration::from_millis(250);

/// Summaries the stream holds for a session that falls behind: four a book.
const FEED_CAPACITY: usize = 4 * crate::watched_books::MAX_WATCHED_BOOKS;

/// The `book_summary` message.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct BookSummary {
    /// The book.
    pub(crate) book_id: BookId,
    /// The book version evaluated.
    pub(crate) book_version: Version,
    /// The limit set version evaluated, while the book holds a non-empty set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) limit_set_version: Option<Version>,
    /// Of two summaries from one monitor, the higher is the later.
    pub(crate) sequence: u64,
    /// When this monitor started; a later start begins its sequence again.
    pub(crate) monitor_started_at: UtcTimestamp,
    /// When it was evaluated.
    pub(crate) evaluated_at: UtcTimestamp,
    /// The book's reporting currency, which its values are in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) currency: Option<convex_core::Currency>,
    /// The runtime's settlement date the values are at.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) settlement_date: Option<BusinessDate>,
    /// Its risk, each measure as a limit on it reports it.
    pub(crate) risk: BookRisk,
    /// Its P&L since its latest close.
    pub(crate) pnl: BookPnl,
    /// Each limit's status of the moment, in set order.
    pub(crate) limits: Vec<LimitStatusNow>,
}

/// A book's P&L since its latest close, as a summary carries it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum BookPnl {
    /// The total over the positions held through, as `GET .../pnl` totals them.
    Measured {
        /// The close's version.
        close_version: Version,
        /// The UTC date of the close.
        close_date: BusinessDate,
        /// P&L since the close, entitled cash included.
        pnl: ExactDecimal,
        /// Of which entitled cash.
        entitled_cash: ExactDecimal,
        /// Positions with no P&L, which the total leaves out.
        excluded_count: usize,
    },
    /// Why there is none.
    Unknown {
        /// The reason.
        reason: NoBookPnlReason,
    },
}

/// A limit's status of the moment, its value and utilisation.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LimitStatusNow {
    /// The limit.
    pub(crate) limit_id: LimitId,
    /// Its status.
    pub(crate) status: LimitStatus,
    /// The measured value, as the status route reports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<ExactDecimal>,
    /// Its utilisation, as the status route reports it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) utilisation: Option<ExactDecimal>,
}

impl LimitStatusNow {
    pub(crate) fn of(evaluation: &LimitEvaluation) -> Self {
        Self {
            limit_id: evaluation.limit_id.clone(),
            status: evaluation.status,
            value: evaluation.value,
            utilisation: evaluation.utilisation,
        }
    }
}

/// A summary on its way to subscribers.
pub(crate) struct BookFrame {
    pub(crate) summary: Arc<BookSummary>,
    /// What a subscriber must hold `BookRead` on, as for P&L: each instrument the book
    /// holds, a limit names or its close holds.
    pub(crate) instruments: Arc<[String]>,
}

/// The stream of summaries and each watched book's latest, handed to WebSocket sessions.
#[derive(Clone)]
pub(crate) struct BookFeed {
    sender: broadcast::Sender<Arc<BookFrame>>,
    latest: Arc<Mutex<HashMap<String, Arc<BookFrame>>>>,
}

impl BookFeed {
    pub(crate) fn new() -> Self {
        Self {
            sender: broadcast::channel(FEED_CAPACITY).0,
            latest: Arc::default(),
        }
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<BookFrame>> {
        self.sender.subscribe()
    }

    /// The latest summary of `book`, while it is watched.
    pub(crate) fn latest(&self, book: &str) -> Option<Arc<BookFrame>> {
        self.kept().get(book).cloned()
    }

    /// Keep `frame` as its book's latest.
    fn keep(&self, frame: Arc<BookFrame>) {
        let book = frame.summary.book_id.as_str().to_owned();
        self.kept().insert(book, frame);
    }

    /// Send `frame` to every session following.
    fn send(&self, frame: Arc<BookFrame>) {
        // No session follows the topic: nothing to send to.
        let _ = self.sender.send(frame);
    }

    /// Forget `book`'s latest, once it is no longer watched.
    fn forget(&self, book: &BookId) {
        self.kept().remove(book.as_str());
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<BookFrame>>> {
        // A panic while holding the map leaves it whole: each write is one insert or remove.
        self.latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// What sends a monitor's summaries: the feed, each book's interval, and the sequence.
pub(crate) struct SummaryPublisher {
    feed: BookFeed,
    throttle: Throttle<Arc<BookFrame>>,
    sequence: u64,
}

impl SummaryPublisher {
    pub(crate) fn new(feed: BookFeed) -> Self {
        Self {
            feed,
            throttle: Throttle::new(SUMMARY_INTERVAL),
            sequence: 0,
        }
    }

    /// The next summary's sequence.
    pub(crate) fn next_sequence(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }

    /// Keep `frame` as its book's latest, and send it at `now` or when its book's interval
    /// ends.
    pub(crate) fn publish(&mut self, frame: Arc<BookFrame>, now: Instant) {
        self.feed.keep(frame.clone());
        let book = frame.summary.book_id.clone();
        if let Some(frame) = self.throttle.offer(&book, frame, now) {
            self.feed.send(frame);
        }
    }

    /// Send each summary that waited out its book's interval by `now`.
    pub(crate) fn send_due(&mut self, now: Instant) {
        for frame in self.throttle.due(now) {
            self.feed.send(frame);
        }
    }

    /// When the next waiting summary is due.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        self.throttle.next_due()
    }

    /// Forget `book`, which is no longer watched.
    pub(crate) fn forget(&mut self, book: &BookId) {
        self.throttle.forget(book);
        self.feed.forget(book);
    }
}

/// When each book's summaries go out: at most one every `interval`, the latest of those
/// evaluated meanwhile sent when the interval ends.
#[derive(Debug)]
struct Throttle<T> {
    interval: Duration,
    books: HashMap<BookId, Window<T>>,
}

#[derive(Debug)]
struct Window<T> {
    sent_at: Instant,
    waiting: Option<T>,
}

impl<T> Throttle<T> {
    fn new(interval: Duration) -> Self {
        Self {
            interval,
            books: HashMap::new(),
        }
    }

    /// `item`, `book`'s latest at `now`, if it goes out now; otherwise it waits for the
    /// interval's end, in place of any waiting.
    fn offer(&mut self, book: &BookId, item: T, now: Instant) -> Option<T> {
        let interval = self.interval;
        match self.books.get_mut(book) {
            Some(window) if now.saturating_duration_since(window.sent_at) < interval => {
                window.waiting = Some(item);
                None
            }
            _ => {
                self.books.insert(
                    book.clone(),
                    Window {
                        sent_at: now,
                        waiting: None,
                    },
                );
                Some(item)
            }
        }
    }

    /// Each waiting item whose interval has ended by `now`, which goes out now.
    fn due(&mut self, now: Instant) -> Vec<T> {
        let interval = self.interval;
        self.books
            .values_mut()
            .filter(|window| now.saturating_duration_since(window.sent_at) >= interval)
            .filter_map(|window| {
                let item = window.waiting.take()?;
                window.sent_at = now;
                Some(item)
            })
            .collect()
    }

    /// When the next waiting item is due.
    fn next_due(&self) -> Option<Instant> {
        self.books
            .values()
            .filter(|window| window.waiting.is_some())
            .map(|window| window.sent_at + self.interval)
            .min()
    }

    /// Forget `book`, which is no longer watched.
    fn forget(&mut self, book: &BookId) {
        self.books.remove(book);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn book(id: &str) -> BookId {
        BookId::new(id).unwrap()
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn a_first_summary_goes_out_at_once_and_a_burst_sends_its_last_at_the_interval() {
        let (a, start) = (book("A"), Instant::now());
        let mut throttle = Throttle::new(ms(250));
        assert_eq!(throttle.offer(&a, 1, start), Some(1));
        assert_eq!(throttle.next_due(), None);
        assert_eq!(throttle.offer(&a, 2, start + ms(50)), None);
        assert_eq!(throttle.offer(&a, 3, start + ms(100)), None);
        assert_eq!(throttle.next_due(), Some(start + ms(250)));
        assert!(throttle.due(start + ms(249)).is_empty());
        assert_eq!(throttle.due(start + ms(250)), [3]);
        assert!(throttle.due(start + ms(600)).is_empty());
        assert_eq!(throttle.next_due(), None);
        // The interval runs from the last sent: one at 400 waits until 500.
        assert_eq!(throttle.offer(&a, 4, start + ms(400)), None);
        assert_eq!(throttle.next_due(), Some(start + ms(500)));
        assert_eq!(throttle.due(start + ms(500)), [4]);
        // Once the interval has passed with nothing waiting, the next goes at once.
        assert_eq!(throttle.offer(&a, 5, start + ms(750)), Some(5));
    }

    #[test]
    fn each_book_has_its_own_interval_and_a_forgotten_book_starts_afresh() {
        let (a, b, start) = (book("A"), book("B"), Instant::now());
        let mut throttle = Throttle::new(ms(250));
        assert_eq!(throttle.offer(&a, 1, start), Some(1));
        assert_eq!(throttle.offer(&b, 10, start + ms(100)), Some(10));
        assert_eq!(throttle.offer(&a, 2, start + ms(150)), None);
        assert_eq!(throttle.offer(&b, 11, start + ms(200)), None);
        assert_eq!(throttle.next_due(), Some(start + ms(250)));
        assert_eq!(throttle.due(start + ms(250)), [2]);
        assert_eq!(throttle.next_due(), Some(start + ms(350)));
        throttle.forget(&b);
        assert_eq!(throttle.next_due(), None);
        assert_eq!(throttle.offer(&b, 12, start + ms(300)), Some(12));
    }
}
