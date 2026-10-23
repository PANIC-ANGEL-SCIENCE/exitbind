include!("release_integrity/context.rs");

#[path = "release_integrity/doc_versions.rs"]
mod doc_versions;
#[path = "release_integrity/installer_asset.rs"]
mod installer_asset;
#[path = "release_integrity/provisional_bases.rs"]
mod provisional_bases;
#[path = "release_integrity/publication_workflows.rs"]
mod publication_workflows;
#[path = "release_integrity/readme_only_gates.rs"]
mod readme_only_gates;
#[path = "release_integrity/record_fields.rs"]
mod record_fields;
#[path = "release_integrity/release_policy.rs"]
mod release_policy;
#[path = "inventory/rust_source.rs"]
mod rust_source;
#[path = "release_integrity/source_and_history_integrity.rs"]
mod source_and_history_integrity;
