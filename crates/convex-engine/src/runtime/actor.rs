//! Single writer for accepted events, exact dependency recovery, result/outbox state and input health.
use super::*;
pub(super) struct Actor {
    services: RuntimeServices,
    assembler: SnapshotAssembler,
    graph: Arc<CalculationGraph>,
    valuator: Arc<crate::snapshot::SnapshotValuator>,
    concurrency: usize,
    checkpoint: RuntimeCheckpoint,
    live: Option<StreamReceiver<QuoteEvent>>,
    health: Arc<parking_lot::RwLock<RuntimeHealth>>,
    results: Arc<parking_lot::RwLock<Vec<PublishedValuation>>>,
    updates: broadcast::Sender<PublishedValuation>,
    live_source: Option<Arc<dyn convex_ports::market_data::QuoteSource>>,
    data_deadline: Option<tokio::time::Instant>,
    needs_images: std::collections::HashSet<convex_ports::quote_event::QuoteKey>,
}
impl Actor {
    pub(super) async fn restore(
        services: RuntimeServices,
        engine: &crate::PricingEngine,
        health: Arc<parking_lot::RwLock<RuntimeHealth>>,
        results: Arc<parking_lot::RwLock<Vec<PublishedValuation>>>,
        updates: broadcast::Sender<PublishedValuation>,
    ) -> Result<Self, RuntimeError> {
        let digest = services.plan.digest()?;
        let input_matches = matches!(
            (&services.input, services.plan.mode),
            (RuntimeInput::Captured, RuntimeMode::Captured)
                | (RuntimeInput::Replay(_), RuntimeMode::Replay)
                | (RuntimeInput::Live, RuntimeMode::Live)
        );
        if !input_matches
            || !(1..=64).contains(&engine.config.max_concurrent_calcs)
            || engine.calc_graph.node_count() != 0
            || engine.config.sharding.enabled
        {
            return Err(RuntimeError::Configuration(
                "runtime input/worker/graph/sharding configuration is unsupported",
            ));
        }
        let loaded = bounded(services.store.load(&services.plan.runtime_id)).await?;
        let absent = loaded.is_none();
        let checkpoint = loaded.unwrap_or_else(|| RuntimeCheckpoint {
            schema_version: 1,
            runtime_id: services.plan.runtime_id.clone(),
            revision: 1,
            plan_digest: digest.clone(),
            generation: services.plan.assembly.generation,
            as_of: services.plan.assembly.as_of,
            known_at: services.plan.assembly.known_at,
            events: services.plan.seed_events.clone(),
            snapshot_id: None,
            pending: true,
            result_revision: 0,
            results: vec![],
            outbox: vec![],
            delivery_lost: false,
        });
        checkpoint.validate()?;
        if checkpoint.plan_digest != digest || checkpoint.runtime_id != services.plan.runtime_id {
            return Err(RuntimeError::RecoveryConflict);
        }
        if absent {
            bounded(services.store.checkpoint(0, &checkpoint)).await?;
        }
        let assembler = SnapshotAssembler::new(
            engine.storage.bonds.clone(),
            engine.storage.configs.clone(),
            services.fixings.clone(),
            services.snapshots.clone(),
        );
        let live_source =
            (services.plan.mode == RuntimeMode::Live).then(|| engine.market_data.quotes.clone());
        let needs_images = if live_source.is_some() {
            services
                .plan
                .assembly
                .quotes
                .iter()
                .map(|q| q.key.clone())
                .collect()
        } else {
            Default::default()
        };
        let mut actor = Self {
            valuator: Arc::new(crate::snapshot::SnapshotValuator::new(
                services.plan.build_id.clone(),
                256,
            )?),
            services,
            assembler,
            graph: engine.calc_graph.clone(),
            concurrency: engine.config.max_concurrent_calcs,
            checkpoint,
            live: None,
            live_source,
            data_deadline: None,
            health,
            results,
            updates,
            needs_images,
        };
        // Reopen replay from the beginning, verify its accepted prefix, and only then continue.
        if let RuntimeInput::Replay(source) = &mut actor.services.input {
            for expected in &actor.checkpoint.events {
                let actual = tokio::time::timeout(IO_BUDGET, source.next_event())
                    .await
                    .map_err(|_| RuntimeError::Deadline)?
                    .map_err(|_| RuntimeError::Source)?;
                if actual != *expected {
                    return Err(RuntimeError::RecoveryConflict);
                }
            }
        }
        actor.restore_dependencies().await?;
        actor.flush_outbox().await?;
        if let Some(source) = &actor.live_source {
            let ids: Vec<_> = actor
                .services
                .plan
                .assembly
                .instruments
                .iter()
                .map(|p| convex_core::ids::InstrumentId::from(p.instrument_id.clone()))
                .collect();
            actor.live = Some(bounded(source.subscribe(&ids)).await?);
        }
        actor.refresh_health();
        Ok(actor)
    }
    fn request(&self) -> SnapshotAssembly {
        let mut request = self.services.plan.assembly.clone();
        request.generation = self.checkpoint.generation;
        request.as_of = self.checkpoint.as_of;
        request.known_at = self.checkpoint.known_at;
        request
    }
    async fn save(&mut self, mut next: RuntimeCheckpoint) -> Result<(), RuntimeError> {
        next.revision = self
            .checkpoint
            .revision
            .checked_add(1)
            .ok_or(RuntimeError::CapacityExceeded)?;
        let saved = bounded(
            self.services
                .store
                .checkpoint(self.checkpoint.revision, &next),
        )
        .await;
        // Timeout/unknown outcome may have committed; reconcile exact content before accepting or retrying.
        if let Err(error) = saved {
            if bounded(self.services.store.load(&next.runtime_id))
                .await?
                .as_ref()
                != Some(&next)
            {
                return Err(error);
            }
        }
        self.checkpoint = next;
        Ok(())
    }
    async fn restore_dependencies(&mut self) -> Result<(), RuntimeError> {
        let snapshot = Arc::new(
            bounded(
                self.assembler
                    .assemble(&self.request(), &self.checkpoint.events),
            )
            .await?,
        );
        if !self.checkpoint.pending && self.checkpoint.snapshot_id.as_ref() != Some(snapshot.id()) {
            return Err(RuntimeError::RecoveryConflict);
        }
        if !self.checkpoint.pending {
            let id = self
                .checkpoint
                .snapshot_id
                .as_ref()
                .ok_or(RuntimeError::RecoveryConflict)?;
            if bounded(self.services.snapshots.get(id)).await?.as_ref() != Some(snapshot.as_ref()) {
                return Err(RuntimeError::RecoveryConflict);
            }
        }
        let mut retained = std::collections::BTreeMap::new();
        for record in &self.checkpoint.results {
            if let Some(good) = &record.last_good {
                if good.provenance().snapshot_id != *snapshot.id() {
                    let old = bounded(self.services.snapshots.get(&good.provenance().snapshot_id))
                        .await?
                        .ok_or(RuntimeError::RecoveryConflict)?;
                    if old.content().generation >= snapshot.content().generation {
                        return Err(RuntimeError::RecoveryConflict);
                    }
                    retained
                        .entry(old.content().generation)
                        .or_insert_with(|| (Arc::new(old), Vec::new()))
                        .1
                        .push(good.clone());
                }
            }
        }
        for (_, (old, records)) in retained {
            worker::activate(&self.graph, &self.services.plan, old.clone())?;
            for good in records {
                worker::restore_good(&self.graph, &self.services.plan, &old, &good)?;
            }
        }
        worker::activate(&self.graph, &self.services.plan, snapshot.clone())?;
        self.set_deadline(&snapshot)?;
        if self.checkpoint.pending {
            self.calculate(snapshot).await?;
        } else {
            worker::restore_results(
                &self.graph,
                &self.services.plan,
                &snapshot,
                &self.checkpoint.results,
            )?;
        }
        *self.results.write() = self.checkpoint.results.clone();
        Ok(())
    }
    async fn accept(
        &mut self,
        event: QuoteEvent,
        as_of: UtcTimestamp,
        known_at: UtcTimestamp,
    ) -> Result<AcceptedUpdate, RuntimeError> {
        event
            .validate()
            .map_err(|_| RuntimeError::Configuration("invalid normalized event"))?;
        if self.checkpoint.pending || !self.checkpoint.outbox.is_empty() {
            return Err(RuntimeError::State("prior accepted work requires recovery"));
        }
        if !self
            .services
            .plan
            .assembly
            .quotes
            .iter()
            .any(|q| q.key == event.key)
            || as_of < self.checkpoint.as_of
            || known_at < self.checkpoint.known_at
            || event.source_time_ms > as_of.datetime().timestamp_millis()
            || event.receive_time_ms > known_at.datetime().timestamp_millis()
            || self
                .checkpoint
                .events
                .last()
                .is_some_and(|v| v.receive_time_ms > event.receive_time_ms)
        {
            return Err(RuntimeError::Configuration(
                "event/cutoff differs from declared ordered input universe",
            ));
        }
        if self.checkpoint.events.len() == MAX_RUNTIME_EVENTS {
            return Err(RuntimeError::CapacityExceeded);
        }
        let mut next = self.checkpoint.clone();
        next.generation = Version::new(
            next.generation
                .get()
                .checked_add(1)
                .ok_or(RuntimeError::CapacityExceeded)?,
        )
        .map_err(|_| RuntimeError::CapacityExceeded)?;
        next.as_of = as_of;
        next.known_at = known_at;
        next.events.push(event.clone());
        next.pending = true;
        if matches!(
            event.event,
            convex_ports::quote_event::QuoteEventKind::Image { .. }
        ) {
            self.needs_images.remove(&event.key);
        }
        if self.needs_images.is_empty() {
            next.delivery_lost = false;
        }
        self.save(next).await?;
        {
            let mut health = self.health.write();
            health.ready = false;
            health.phase = RuntimePhase::Degraded;
            health.generation = self.checkpoint.generation.get();
        }
        Ok(AcceptedUpdate {
            checkpoint_revision: self.checkpoint.revision,
            generation: self.checkpoint.generation,
        })
    }
    async fn process(&mut self) -> Result<(), RuntimeError> {
        let snapshot = Arc::new(
            bounded(
                self.assembler
                    .assemble(&self.request(), &self.checkpoint.events),
            )
            .await?,
        );
        worker::activate(&self.graph, &self.services.plan, snapshot.clone())?;
        self.calculate(snapshot).await?;
        self.flush_outbox().await?;
        self.refresh_health();
        Ok(())
    }
    async fn calculate(&mut self, snapshot: Arc<MarketSnapshot>) -> Result<(), RuntimeError> {
        self.set_deadline(&snapshot)?;
        let produced = worker::calculate(
            &self.graph,
            &self.valuator,
            &self.services.plan,
            snapshot.clone(),
            &self.checkpoint,
            self.concurrency,
        )
        .await?;
        let mut next = self.checkpoint.clone();
        next.result_revision = produced.last().map_or(next.result_revision, |p| p.revision);
        next.results = produced.clone();
        next.outbox = produced;
        next.pending = false;
        next.snapshot_id = Some(snapshot.id().clone());
        self.save(next).await?;
        *self.results.write() = self.checkpoint.results.clone();
        Ok(())
    }
    async fn flush_outbox(&mut self) -> Result<(), RuntimeError> {
        // Whole bounded batch retains its original delivery keys on retry; consumers deduplicate.
        for record in &self.checkpoint.outbox {
            tokio::time::timeout(IO_BUDGET, self.services.publisher.publish(record))
                .await
                .map_err(|_| RuntimeError::Deadline)?
                .map_err(|_| RuntimeError::Publication)?;
        }
        if !self.checkpoint.outbox.is_empty() {
            let delivered = self.checkpoint.outbox.clone();
            let mut next = self.checkpoint.clone();
            next.outbox.clear();
            self.save(next).await?;
            self.refresh_health();
            for record in delivered {
                let _ = self.updates.send(record);
            }
        }
        Ok(())
    }
    fn set_deadline(&mut self, snapshot: &MarketSnapshot) -> Result<(), RuntimeError> {
        self.data_deadline = if self.services.plan.mode == RuntimeMode::Live
            && snapshot.quality().quality == DataQuality::Valid
        {
            worker::expiry(snapshot)?.map(|deadline| {
                let delta = deadline
                    .datetime()
                    .signed_duration_since(chrono::Utc::now())
                    .to_std()
                    .unwrap_or(Duration::ZERO);
                tokio::time::Instant::now() + delta + Duration::from_nanos(1)
            })
        } else {
            None
        };
        Ok(())
    }
    async fn expire_data(&mut self) -> Result<(), RuntimeError> {
        let now =
            UtcTimestamp::from_datetime(chrono::Utc::now()).map_err(|_| RuntimeError::Source)?;
        let mut next = self.checkpoint.clone();
        next.as_of = now;
        next.known_at = now;
        next.generation = Version::new(
            next.generation
                .get()
                .checked_add(1)
                .ok_or(RuntimeError::CapacityExceeded)?,
        )
        .map_err(|_| RuntimeError::CapacityExceeded)?;
        next.pending = true;
        self.save(next).await?;
        self.process().await
    }
    async fn lose_delivery(&mut self) -> Result<(), RuntimeError> {
        let mut next = self.checkpoint.clone();
        next.delivery_lost = true;
        let mut records = Vec::with_capacity(next.results.len());
        for prior in &next.results {
            next.result_revision = next
                .result_revision
                .checked_add(1)
                .ok_or(RuntimeError::CapacityExceeded)?;
            records.push(worker::lost_record(prior, next.result_revision)?);
        }
        next.results = records.clone();
        next.outbox = records;
        self.save(next).await?;
        *self.results.write() = self.checkpoint.results.clone();
        self.refresh_health();
        self.flush_outbox().await
    }
    fn refresh_health(&self) {
        let expired = self
            .data_deadline
            .is_some_and(|deadline| tokio::time::Instant::now() >= deadline);
        let quality = if expired {
            DataQuality::Stale
        } else {
            self.checkpoint
                .results
                .iter()
                .map(|r| r.result.quality())
                .find(|q| *q != DataQuality::Valid)
                .unwrap_or(DataQuality::Valid)
        };
        let complete = !self.checkpoint.results.is_empty()
            && self
                .checkpoint
                .results
                .iter()
                .all(|r| r.result.status() == ResultStatus::Complete);
        let mut health = self.health.write();
        let ready = complete
            && quality == DataQuality::Valid
            && !self.checkpoint.delivery_lost
            && self.needs_images.is_empty()
            && self.checkpoint.outbox.is_empty()
            && !self.checkpoint.pending;
        if health.phase != RuntimePhase::Draining {
            health.phase = if ready {
                RuntimePhase::Ready
            } else {
                RuntimePhase::Degraded
            };
            health.ready = ready;
        }
        health.mode = Some(self.services.plan.mode);
        health.quality = quality;
        health.snapshot_id = self.checkpoint.snapshot_id.clone();
        health.generation = self.checkpoint.generation.get();
        health.revision = self.checkpoint.result_revision;
        health.pending_publications = self.checkpoint.outbox.len();
    }
    async fn next_event(&mut self) -> Result<QuoteEvent, StreamError> {
        match &mut self.services.input {
            RuntimeInput::Replay(source) => source.next_event().await,
            RuntimeInput::Live => match self.live.as_mut() {
                Some(source) => source.recv().await,
                None => Err(StreamError::Closed),
            },
            RuntimeInput::Captured => std::future::pending().await,
        }
    }
    pub(super) async fn run(
        mut self,
        mut rx: mpsc::Receiver<Command>,
        mut stop: watch::Receiver<bool>,
    ) -> Result<(), RuntimeError> {
        let mut outcome = self.run_inputs(&mut rx, &mut stop).await;
        if let Some(source) = &self.live_source {
            let ids: Vec<_> = self
                .services
                .plan
                .assembly
                .instruments
                .iter()
                .map(|p| convex_core::ids::InstrumentId::from(p.instrument_id.clone()))
                .collect();
            if let Err(error) = bounded(source.unsubscribe(&ids)).await {
                if outcome.is_ok() {
                    outcome = Err(error);
                }
            }
        }
        if let Err(error) = &outcome {
            if let Err(fence) = self.graph.suspend_market_delivery() {
                tracing::error!("Runtime graph suspension failed: {fence}");
            }
            let mut health = self.health.write();
            health.phase = RuntimePhase::Failed;
            health.ready = false;
            health.failure = Some(error.to_string());
            health.pending_publications = self.checkpoint.outbox.len();
        }
        rx.close();
        while let Ok(command) = rx.try_recv() {
            let _ = command.reply.send(Err(RuntimeError::State(
                "input not accepted before runtime failure",
            )));
        }
        outcome
    }
    async fn run_inputs(
        &mut self,
        rx: &mut mpsc::Receiver<Command>,
        stop: &mut watch::Receiver<bool>,
    ) -> Result<(), RuntimeError> {
        let mut source_finished = false;
        loop {
            let deadline = self
                .data_deadline
                .unwrap_or_else(|| tokio::time::Instant::now() + Duration::from_secs(86400));
            tokio::select! {
                biased;
                changed = stop.changed() => {
                    if changed.is_err() || *stop.borrow() {
                        rx.close();
                        while let Some(command) = rx.recv().await { self.process_command(command).await?; }
                        self.flush_outbox().await?;
                        return Ok(());
                    }
                }
                command = rx.recv() => match command { Some(command) => self.process_command(command).await?, None => { self.flush_outbox().await?; return Ok(()); } },
                _ = tokio::time::sleep_until(deadline), if self.data_deadline.is_some() => { self.expire_data().await?; }
                event = self.next_event(), if !source_finished => {
                    match event {
                        Ok(event) => {
                            let (as_of, known_at) = if self.services.plan.mode == RuntimeMode::Live {
                                let now = UtcTimestamp::from_datetime(chrono::Utc::now()).map_err(|_| RuntimeError::Source)?;
                                (now, now)
                            } else {
                                let source = UtcTimestamp::from_datetime(chrono::DateTime::from_timestamp_millis(event.source_time_ms).ok_or(RuntimeError::Source)?).map_err(|_| RuntimeError::Source)?;
                                let receipt = UtcTimestamp::from_datetime(chrono::DateTime::from_timestamp_millis(event.receive_time_ms).ok_or(RuntimeError::Source)?).map_err(|_| RuntimeError::Source)?;
                                (self.checkpoint.as_of.max(source), self.checkpoint.known_at.max(receipt))
                            };
                            self.accept(event, as_of, known_at).await?; self.process().await?;
                        }
                        Err(StreamError::Closed) if self.services.plan.mode == RuntimeMode::Replay => { source_finished = true; self.health.write().replay_complete = true; }
                        Err(_) => {
                            self.lose_delivery().await?;
                            return Err(RuntimeError::Source);
                        }
                    }
                }
            }
        }
    }
    async fn process_command(&mut self, command: Command) -> Result<(), RuntimeError> {
        match self
            .accept(command.event, command.as_of, command.known_at)
            .await
        {
            Ok(receipt) => {
                let _ = command.reply.send(Ok(receipt));
                self.process().await
            }
            Err(error @ (RuntimeError::Configuration(_) | RuntimeError::CapacityExceeded)) => {
                let _ = command.reply.send(Err(error));
                Ok(())
            }
            Err(error) => {
                let _ = command.reply.send(Err(error.clone()));
                Err(error)
            }
        }
    }
}
