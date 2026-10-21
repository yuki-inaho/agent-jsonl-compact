//! Single source of release metadata: semver comes from Cargo, the release date is a fixed
//! reproducible source date (never a build timestamp).

pub const SOURCE_RELEASE_DATE: &str = "2026-10-21";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn json() -> serde_json::Value {
    serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": VERSION,
        "source_release_date": SOURCE_RELEASE_DATE,
        "query_schema_version": crate::query::model::QUERY_SCHEMA_VERSION,
        "cursor_version": crate::query::cursor::CURSOR_VERSION,
        "source_date_kind": "fixed_release_date_not_build_time",
    })
}

pub fn text() -> String {
    format!(
        "agent-jsonl-compact {VERSION}\nsource_release_date: {SOURCE_RELEASE_DATE}\nquery_schema_version: {}\n",
        crate::query::model::QUERY_SCHEMA_VERSION
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_cargo_and_date_is_fixed() {
        assert_eq!(super::VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(super::SOURCE_RELEASE_DATE, "2026-10-21");
        let json = super::json();
        assert_eq!(json["version"], super::VERSION);
        assert_eq!(json["query_schema_version"], 1);
    }
}
