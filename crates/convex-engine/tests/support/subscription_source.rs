#![allow(dead_code)]
use async_trait::async_trait;
use convex_ports::{market_data::*, quote_event::QuoteEvent, TraitError};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
/// Receipt time in wall-clock milliseconds that never steps back in this process, as the
/// server ingress stamps it (D70). A raw clock reading after a backward step would be
/// refused by the Live runtime, which then stops.
pub fn monotone_stamp() -> i64 {
    static LAST: AtomicI64 = AtomicI64::new(i64::MIN);
    let now = chrono::Utc::now().timestamp_millis();
    LAST.fetch_max(now, Ordering::SeqCst).max(now)
}
pub struct SubscriptionSource {
    sender: std::sync::Mutex<Option<tokio::sync::broadcast::Sender<QuoteEvent>>>,
    pub unsubscribed: AtomicBool,
}
impl SubscriptionSource {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            sender: std::sync::Mutex::new(Some(tx)),
            unsubscribed: AtomicBool::new(false),
        }
    }
    pub fn send(&self, event: QuoteEvent) {
        self.sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(event)
            .unwrap();
    }
    pub fn close(&self) {
        self.sender.lock().unwrap().take();
    }
}
#[async_trait]
impl QuoteSource for SubscriptionSource {
    fn source_type(&self) -> SourceType {
        SourceType::Streaming
    }
    async fn get_quote(
        &self,
        _: &convex_core::ids::InstrumentId,
    ) -> Result<Option<RawQuote>, TraitError> {
        Err(TraitError::Unsupported(
            "canonical subscription fixture only",
        ))
    }
    async fn get_quotes(
        &self,
        _: &[convex_core::ids::InstrumentId],
    ) -> Result<Vec<RawQuote>, TraitError> {
        Err(TraitError::Unsupported(
            "canonical subscription fixture only",
        ))
    }
    async fn subscribe(
        &self,
        ids: &[convex_core::ids::InstrumentId],
    ) -> Result<StreamReceiver<QuoteEvent>, TraitError> {
        assert!(ids.contains(&convex_core::ids::InstrumentId::new("BOND-001")));
        Ok(StreamReceiver::new(
            self.sender.lock().unwrap().as_ref().unwrap().subscribe(),
        ))
    }
    async fn unsubscribe(&self, _: &[convex_core::ids::InstrumentId]) -> Result<(), TraitError> {
        self.unsubscribed.store(true, Ordering::SeqCst);
        Ok(())
    }
}
