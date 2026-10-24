#![allow(dead_code)] // Engine and HTTP suites use different parts of the same captured setup.
use convex::library::*;
use convex_core::contracts::*;
use convex_engine::{
    core_workflow::CoreWorkflow,
    ports::{
        core_workflow::*, fixing::*, reference_master::*, reference_repository::*, snapshot::*,
        storage::StorageAdapter,
    },
};
use convex_ext_file::FileSnapshotRepository;
use convex_ext_redb::{RedbFixingRepository, RedbRuntimeStore};
use std::{collections::BTreeMap, path::Path, sync::Arc};

pub fn time() -> UtcTimestamp {
    UtcTimestamp::parse("2026-10-07T12:00:00Z").unwrap()
}
pub struct Fixture {
    pub storage: Arc<StorageAdapter>,
    pub results: Arc<RedbRuntimeStore>,
    pub fixings: Arc<RedbFixingRepository>,
    pub snapshots: Arc<FileSnapshotRepository>,
    pub plan: CoreSnapshotPlan,
    pub request: LibraryRequest,
    pub published: Vec<PublishedFixing>,
    pub recipe: SnapshotCurveRecipe,
}
impl Fixture {
    pub async fn new(directory: &Path) -> Self {
        let (storage, results, fixings) =
            convex_ext_redb::create_redb_storage_with_core(directory.join("state.redb")).unwrap();
        let storage = Arc::new(storage);
        let snapshots = Arc::new(FileSnapshotRepository::new(directory.join("snapshots")).unwrap());
        let file = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/library/graph-hedge-qualified.json");
        let cases: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let case = cases["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "graph_hedge.positive.bond.base")
            .unwrap();
        let request: LibraryRequest = serde_json::from_value(case["request"].clone()).unwrap();
        let mut reference = request.inputs.clone();
        let mut pins = Vec::new();
        let mut recipes = Vec::new();
        for recipe in &reference.curves {
            let recipe = SnapshotCurveRecipe {
                schema_version: 1,
                curve_id: CurveId::new(recipe.key.id.clone()).unwrap(),
                version: Version::new(1).unwrap(),
                source_time: time(),
                recorded_at: time(),
                source: SourceId::new("core.fixture.curves").unwrap(),
                model_id: ModelId::new("native.supplied").unwrap(),
                input_versions: vec![],
                quality: DataQuality::Demo,
                max_age_ms: 86_400_000,
                recipe: serde_json::to_value(recipe).unwrap(),
            };
            storage.curves.save_recipe(&recipe).await.unwrap();
            pins.push(ReferencePin {
                id: ReferenceDataId::new(recipe.curve_id.as_str()).unwrap(),
                version: recipe.version,
            });
            recipes.push(recipe);
        }
        let published: Vec<_> = reference
            .fixings
            .iter()
            .map(|f| PublishedFixing {
                schema_version: 1,
                key: FixingKey {
                    index_id: f.index_id.clone(),
                    observation_date: BusinessDate::parse(&f.observation_date.to_string()).unwrap(),
                    source: f.source_id.clone(),
                },
                version: f.revision,
                rate: DecimalRate::new(ExactDecimal::new(f.rate)),
                published_at: f.published_at,
                recorded_at: f.known_at,
                is_preliminary: f.status == convex_core::fixings::FixingStatus::Preliminary,
                entitled: true,
                evidence_reference: "existing synthetic native fixture".into(),
            })
            .collect();
        fixings.publish(&published).await.unwrap();
        reference.curves.clear();
        reference.fixings.clear();
        let mut shell = request.clone();
        shell.inputs = LibraryInputs::empty();
        let id = ReferenceDataId::new("core.test.book").unwrap();
        let definition = CoreDefinition {
            id: id.clone(),
            reference_inputs: serde_json::to_value(reference).unwrap(),
            calculations: BTreeMap::from([(
                "risk".into(),
                CoreCalculation {
                    request: serde_json::to_value(shell).unwrap(),
                    curve_sets: vec!["base".into()],
                },
            )]),
            effective: EffectiveInterval {
                from: UtcTimestamp::parse("2026-01-01T00:00:00Z").unwrap(),
                until: None,
            },
            provenance: ReferenceProvenance {
                source: SourceId::new("core.fixture.reference").unwrap(),
                evidence_reference: "existing native full contracts".into(),
            },
        };
        storage
            .bonds
            .commit(
                &ReferenceCommand {
                    idempotency_key: RequestId::new("core.test.seed").unwrap(),
                    changes: vec![],
                    master_changes: vec![MasterChange {
                        expected_version: None,
                        definition: MasterDefinition::Calculation(Box::new(definition)),
                    }],
                },
                time(),
            )
            .await
            .unwrap();
        let plan = CoreSnapshotPlan {
            definition: ReferencePin {
                id,
                version: Version::new(1).unwrap(),
            },
            as_of: time(),
            known_at: time(),
            curve_sets: BTreeMap::from([("base".into(), pins)]),
            fixings: published.iter().map(|f| f.key.clone()).collect(),
        };
        Self {
            storage,
            results,
            fixings,
            snapshots,
            plan,
            request,
            published,
            recipe: recipes.remove(0),
        }
    }
    pub fn workflow(&self) -> Arc<CoreWorkflow> {
        Arc::new(CoreWorkflow::new(
            self.storage.bonds.clone(),
            self.storage.curves.clone(),
            self.fixings.clone(),
            self.snapshots.clone(),
            self.results.clone(),
            ModelId::new("core.test.build").unwrap(),
        ))
    }
}
