//! Cancellation must not release capacity while a blocking calculation is queued.
#[path = "../../convex-engine/tests/support/runtime_setup.rs"]
mod support;
use super::*;
use crate::{access::AccessControl, websocket::WebSocketState};

struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if let Some(release) = self.0.take() {
            let _ = release.send(());
        }
    }
}

#[test]
fn cancelled_waiter_retains_both_admissions_until_blocking_work_finishes() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap()
        .block_on(async {
            let setup = support::setup().await;
            let engine = support::make_engine(
                &setup,
                support::archive(&setup),
                setup.plan.clone(),
                setup.store.clone(),
                convex_engine::RuntimeInput::Captured,
            );
            let access = AccessControl::local_development();
            let state = Arc::new(AppState {
                analytics_slots: Arc::new(tokio::sync::Semaphore::new(1)),
                access: access.clone(),
                engine,
                ws_state: WebSocketState::new(),
            });
            // Occupy the sole blocking thread. The real calculation queues after
            // acquiring its permit, so cancellation is deterministic without sleeps.
            let (release, blocked) = std::sync::mpsc::channel();
            let release = ReleaseOnDrop(Some(release));
            let blocker = tokio::task::spawn_blocking(move || blocked.recv().unwrap());
            let context = access.authenticate(None).await.unwrap();
            let admission = Arc::new(
                access
                    .admit(&context, false, access.admit_attempt().unwrap())
                    .unwrap(),
            );
            let retained = Arc::downgrade(&admission);
            let input = Bytes::from_static(br#"{"schema_version":999}"#);
            let mut waiting = Box::pin(calculate(
                State(state.clone()),
                Extension(admission),
                input.clone(),
            ));
            assert!(futures_util::poll!(waiting.as_mut()).is_pending());
            drop(waiting);
            assert_eq!(state.analytics_slots.available_permits(), 0);
            assert!(retained.upgrade().is_some());
            let next_admission = Arc::new(
                access
                    .admit(&context, false, access.admit_attempt().unwrap())
                    .unwrap(),
            );
            assert_eq!(
                calculate(
                    State(state.clone()),
                    Extension(next_admission),
                    input.clone()
                )
                .await
                .status(),
                StatusCode::TOO_MANY_REQUESTS
            );
            drop(release);
            blocker.await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while retained.upgrade().is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(state.analytics_slots.available_permits(), 1);
            let admission = Arc::new(
                access
                    .admit(&context, false, access.admit_attempt().unwrap())
                    .unwrap(),
            );
            assert_eq!(
                calculate(State(state), Extension(admission), input)
                    .await
                    .status(),
                StatusCode::OK
            );
        });
}
