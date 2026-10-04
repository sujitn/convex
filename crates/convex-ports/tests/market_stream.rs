use convex_ports::market_data::{StreamError, StreamReceiver};

#[tokio::test]
async fn receiver_must_not_erase_lag() {
    let (tx, rx) = tokio::sync::broadcast::channel(1);
    tx.send(1_u32).unwrap();
    tx.send(2).unwrap();
    let mut rx = StreamReceiver::new(rx);
    assert!(matches!(
        rx.recv().await,
        Err(StreamError::Lagged { missed: 1 })
    ));
    assert_eq!(rx.recv().await.unwrap(), 2);
    drop(tx);
    assert!(matches!(rx.recv().await, Err(StreamError::Closed)));
    assert!(matches!(rx.recv().await, Err(StreamError::Closed)));
}
