// Current Exitbind and preserved historical-reader contracts run in the default suite.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path},
};

const FORMAT_MARKER: &str = "x-soulmate-format-version";
const EXITBIND_FORMAT_MARKER: &str = "x-exitbind-format-version";
const V3_SCHEMA_PATH: &str = "schema/run-event-v3.schema.json";
const V4_SCHEMA_PATH: &str = "schema/run-event-v4.schema.json";
const V5_SCHEMA_PATH: &str = "schema/run-event-v5.schema.json";
const BASELINE_PATH: &str = "ledgers/baseline.jsonl";
const BLOCKED_PATH: &str = "ledgers/blocked.jsonl";
const FINAL_PATH: &str = "ledgers/final.jsonl";
const MANIFEST_PATH: &str = "hash-manifest.json";
const CURRENT_FIXTURE_CONFIG: &str = "fixtures/exitbind.json";
const HISTORICAL_FIXTURE_CONFIG: &str = "fixtures/soulmate.json";
const FIXTURE_CONFIGS: &[&str] = &[CURRENT_FIXTURE_CONFIG, HISTORICAL_FIXTURE_CONFIG];
const FIXTURE_INVALID: &str = "fixtures/verification-invalid.json";
const FIXTURE_VALID: &str = "fixtures/verification-valid.json";


#[derive(Clone)]
struct ManifestEntry {
    sha256: String,
    bytes: u64,
}

type ManifestFiles = BTreeMap<String, ManifestEntry>;
type FixtureFiles = BTreeMap<String, Vec<u8>>;
type EventShape = (BTreeSet<String>, BTreeSet<String>);
type EventShapes = BTreeMap<String, EventShape>;

struct BundleSnapshot {
    fixture_paths: BTreeSet<String>,
}
