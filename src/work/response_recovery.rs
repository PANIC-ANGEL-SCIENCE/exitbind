//! Exact executable/config recovery argv for bounded work responses.

use crate::emitted::{Bounded, Route};
use serde_json::Value;
use std::path::Path;

pub(super) fn recovery_command(
    response: &Value,
    config_path: &Path,
    invoked: &str,
    argv_budget: usize,
) -> Bounded {
    let mut route = Route::new(["work", invoked]);
    if invoked == "next" {
        let Some(work) = response.get("work").and_then(Value::as_str) else {
            return crate::emitted::unavailable();
        };
        if work.len() > 128 {
            return crate::emitted::unavailable();
        }
        route = route.words([work]);
    }
    route
        .words(["--full"])
        .config(config_path)
        .bounded(argv_budget)
}
