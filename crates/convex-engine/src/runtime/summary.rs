//! What the Live deadline and health read from the results, kept up record by record
//! (P17c-iii, G62). A tick that values a few pins updates only their entries, so neither
//! the deadline nor health walks every record. [`ResultView::walk`] walks them, and the
//! summary returns what it returns, which a test holds.
use super::worker::accepted;
use convex_core::contracts::{DataQuality, ResultStatus, UtcTimestamp};
use convex_ports::runtime::PublishedValuation;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// What one record contributes to the view.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Facts {
    quality: DataQuality,
    accepted: bool,
    /// Complete and accepted.
    complete: bool,
    /// An accepted record's expiry.
    expiry: Option<UtcTimestamp>,
}

impl Facts {
    fn of(record: &PublishedValuation, demo: bool) -> Self {
        let quality = record.result.quality();
        let accepted = accepted(quality, demo);
        Self {
            quality,
            accepted,
            complete: accepted && record.result.status() == ResultStatus::Complete,
            expiry: if accepted {
                record.live_valid_until
            } else {
                None
            },
        }
    }
}

/// What health and the deadline read from the records.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ResultView {
    /// The first refused quality in plan order, else Demo when any record is Demo, else
    /// Valid.
    pub(super) quality: DataQuality,
    /// The records that are complete and accepted.
    pub(super) accepted: usize,
    /// The earliest expiry among the accepted records.
    pub(super) expiry: Option<UtcTimestamp>,
}

impl ResultView {
    /// Every record walked, as health and the deadline read them before P17c-iii.
    pub(super) fn walk(results: &[Arc<PublishedValuation>], demo: bool) -> Self {
        Self::of(results.iter().map(|record| Facts::of(record, demo)))
    }

    fn of(facts: impl Iterator<Item = Facts> + Clone) -> Self {
        Self {
            quality: facts
                .clone()
                .find(|f| !f.accepted)
                .or_else(|| facts.clone().find(|f| f.quality == DataQuality::Demo))
                .map_or(DataQuality::Valid, |f| f.quality),
            accepted: facts.clone().filter(|f| f.complete).count(),
            expiry: facts.filter_map(|f| f.expiry).min(),
        }
    }
}

/// [`ResultView`] kept up for one set of records. It describes those records only, so a
/// reader checks [`ResultSummary::describes`] and otherwise walks.
pub(super) struct ResultSummary {
    results: Arc<[Arc<PublishedValuation>]>,
    demo: bool,
    /// The refused records' qualities by plan index; the first is the view's quality.
    refused: BTreeMap<usize, DataQuality>,
    /// The records whose quality is Demo.
    demo_records: usize,
    /// The records that are complete and accepted.
    accepted: usize,
    /// The accepted records' expiries, with their plan index.
    expiries: BTreeSet<(UtcTimestamp, usize)>,
}

impl ResultSummary {
    /// Summarize every record.
    pub(super) fn new(results: Arc<[Arc<PublishedValuation>]>, demo: bool) -> Self {
        let mut summary = Self {
            results: Arc::default(),
            demo,
            refused: BTreeMap::new(),
            demo_records: 0,
            accepted: 0,
            expiries: BTreeSet::new(),
        };
        for (index, record) in results.iter().enumerate() {
            summary.add(index, Facts::of(record, demo));
        }
        summary.results = results;
        summary
    }

    /// Whether this summary is of `results`, read with `demo`.
    pub(super) fn describes(&self, results: &Arc<[Arc<PublishedValuation>]>, demo: bool) -> bool {
        Arc::ptr_eq(&self.results, results) && self.demo == demo
    }

    /// Move to `next`, which is `previous` with only the records at `changed` replaced.
    /// Only those entries are updated. When this summary is not of `previous`, or the
    /// two differ in length, every record of `next` is summarized again.
    pub(super) fn advance(
        mut self,
        previous: &Arc<[Arc<PublishedValuation>]>,
        next: Arc<[Arc<PublishedValuation>]>,
        changed: &[usize],
    ) -> Self {
        if !Arc::ptr_eq(&self.results, previous) || previous.len() != next.len() {
            return Self::new(next, self.demo);
        }
        for &index in changed {
            let (Some(old), Some(new)) = (previous.get(index), next.get(index)) else {
                return Self::new(next, self.demo);
            };
            if !self.remove(index, Facts::of(old, self.demo)) {
                return Self::new(next, self.demo);
            }
            self.add(index, Facts::of(new, self.demo));
        }
        self.results = next;
        self
    }

    /// The view of the records, as [`ResultView::walk`] gives it.
    pub(super) fn view(&self) -> ResultView {
        ResultView {
            quality: self.refused.first_key_value().map_or(
                if self.demo_records > 0 {
                    DataQuality::Demo
                } else {
                    DataQuality::Valid
                },
                |(_, &quality)| quality,
            ),
            accepted: self.accepted,
            expiry: self.expiries.first().map(|&(expiry, _)| expiry),
        }
    }

    fn add(&mut self, index: usize, facts: Facts) {
        if !facts.accepted {
            self.refused.insert(index, facts.quality);
        }
        if facts.quality == DataQuality::Demo {
            self.demo_records = self.demo_records.saturating_add(1);
        }
        if facts.complete {
            self.accepted = self.accepted.saturating_add(1);
        }
        if let Some(expiry) = facts.expiry {
            self.expiries.insert((expiry, index));
        }
    }

    /// Take back what `facts` added at `index`. False when the summary does not hold
    /// them; the summary may then be part changed, so the caller summarizes again.
    fn remove(&mut self, index: usize, facts: Facts) -> bool {
        let held = (facts.accepted || self.refused.remove(&index).is_some())
            && facts
                .expiry
                .is_none_or(|expiry| self.expiries.remove(&(expiry, index)));
        let demo_records = if facts.quality == DataQuality::Demo {
            self.demo_records.checked_sub(1)
        } else {
            Some(self.demo_records)
        };
        let accepted = if facts.complete {
            self.accepted.checked_sub(1)
        } else {
            Some(self.accepted)
        };
        match (held, demo_records, accepted) {
            (true, Some(demo_records), Some(accepted)) => {
                self.demo_records = demo_records;
                self.accepted = accepted;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded linear congruential generator, as the P17c-ii script uses.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, bound: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) % bound
        }
    }

    const QUALITIES: [DataQuality; 6] = [
        DataQuality::Valid,
        DataQuality::Indicative,
        DataQuality::Delayed,
        DataQuality::Stale,
        DataQuality::Invalid,
        DataQuality::Demo,
    ];

    fn facts(rng: &mut Lcg, demo: bool) -> Facts {
        let quality = QUALITIES[rng.next(QUALITIES.len() as u64) as usize];
        let accepted = accepted(quality, demo);
        let expiry = (rng.next(3) > 0).then(|| {
            UtcTimestamp::parse(&format!("2026-11-15T10:{:02}:00Z", rng.next(8))).unwrap()
        });
        Facts {
            quality,
            accepted,
            complete: accepted && rng.next(4) > 0,
            expiry: if accepted { expiry } else { None },
        }
    }

    /// The summary over facts alone, as `advance` keeps it: what the walk over the same
    /// facts returns at every step.
    #[test]
    fn a_summary_kept_up_by_record_views_what_walking_every_record_views() {
        for demo in [false, true] {
            let mut rng = Lcg(if demo { 7 } else { 11 });
            for width in [1usize, 2, 5, 64] {
                let mut current: Vec<Facts> = (0..width).map(|_| facts(&mut rng, demo)).collect();
                let mut summary = ResultSummary {
                    results: Arc::default(),
                    demo,
                    refused: BTreeMap::new(),
                    demo_records: 0,
                    accepted: 0,
                    expiries: BTreeSet::new(),
                };
                for (index, f) in current.iter().enumerate() {
                    summary.add(index, *f);
                }
                for _ in 0..2_000 {
                    let count = rng.next(4) as usize;
                    for _ in 0..count {
                        let index = rng.next(width as u64) as usize;
                        let replacement = facts(&mut rng, demo);
                        assert!(summary.remove(index, current[index]));
                        summary.add(index, replacement);
                        current[index] = replacement;
                    }
                    assert_eq!(
                        summary.view(),
                        ResultView::of(current.iter().copied()),
                        "width {width}, demo {demo}"
                    );
                }
            }
        }
    }

    /// Removing facts the summary does not hold is refused, so `advance` summarizes again
    /// rather than keeping counts that no longer match.
    #[test]
    fn removing_facts_the_summary_does_not_hold_is_refused() {
        let held = Facts {
            quality: DataQuality::Stale,
            accepted: false,
            complete: false,
            expiry: None,
        };
        let mut summary = ResultSummary {
            results: Arc::default(),
            demo: false,
            refused: BTreeMap::new(),
            demo_records: 0,
            accepted: 0,
            expiries: BTreeSet::new(),
        };
        summary.add(0, held);
        assert!(!summary.remove(1, held), "no record at 1");
        let complete = Facts {
            quality: DataQuality::Valid,
            accepted: true,
            complete: true,
            expiry: None,
        };
        assert!(
            !summary.remove(0, complete),
            "the record at 0 is not complete"
        );
        assert!(summary.remove(0, held));
    }
}
