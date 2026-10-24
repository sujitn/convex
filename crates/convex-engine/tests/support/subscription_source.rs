#![allow(dead_code)]
use async_trait::async_trait;
use convex_ports::{market_data::*, quote_event::QuoteEvent, TraitError};
use std::sync::atomic::{AtomicBool, Ordering};
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
        assert_eq!(ids, &[convex_core::ids::InstrumentId::new("BOND-001")]);
        Ok(StreamReceiver::new(
            self.sender.lock().unwrap().as_ref().unwrap().subscribe(),
        ))
    }
    async fn unsubscribe(&self, _: &[convex_core::ids::InstrumentId]) -> Result<(), TraitError> {
        self.unsubscribed.store(true, Ordering::SeqCst);
        Ok(())
    }
}
