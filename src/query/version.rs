//! Single source of release metadata: semver comes from Cargo, the release date is a fixed
//! reproducible source date (never a build timestamp).

pub const SOURCE_RELEASE_DATE: &str = "2026-10-28";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Commit whose source produced this binary. `unknown` when the build had no Git metadata.
pub const SOURCE_COMMIT: &str = match option_env!("AJC_SOURCE_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};
/// Build-time worktree state. `None` means Git was unavailable, which is reported as unknown
/// rather than as a clean worktree.
pub fn source_dirty() -> Option<bool> {
    match option_env!("AJC_SOURCE_DIRTY") {
        Some("true") => Some(true),
        Some("false") => Some(false),
        _ => None,
    }
}

pub fn json() -> serde_json::Value {
    serde_json::json!({
        "name": env!("CARGO_PKG_NAME"),
        "version": VERSION,
        "source_release_date": SOURCE_RELEASE_DATE,
        "source_commit": SOURCE_COMMIT,
        "source_commit_dirty": source_dirty(),
        "query_schema_version": crate::query::model::QUERY_SCHEMA_VERSION,
        "cursor_version": crate::query::cursor::CURSOR_VERSION,
        "source_date_kind": "fixed_release_date_not_build_time",
    })
}

pub fn text() -> String {
    let dirty = match source_dirty() {
        Some(true) => " (dirty)",
        Some(false) => "",
        None => " (dirty unknown)",
    };
    format!(
        "agent-jsonl-compact {VERSION}\nsource_release_date: {SOURCE_RELEASE_DATE}\nsource_commit: {SOURCE_COMMIT}{dirty}\nquery_schema_version: {}\n",
        crate::query::model::QUERY_SCHEMA_VERSION
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_matches_cargo_and_date_is_fixed() {
        assert_eq!(super::VERSION, env!("CARGO_PKG_VERSION"));
        assert_eq!(super::SOURCE_RELEASE_DATE, "2026-10-28");
        assert!(
            super::SOURCE_COMMIT == "unknown"
                || matches!(super::SOURCE_COMMIT.len(), 40 | 64)
                    && super::SOURCE_COMMIT
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit())
        );
        let json = super::json();
        assert_eq!(json["version"], super::VERSION);
        assert_eq!(json["source_commit"], super::SOURCE_COMMIT);
        assert_eq!(
            json["source_commit_dirty"],
            serde_json::to_value(super::source_dirty()).unwrap()
        );
        assert_eq!(json["query_schema_version"], 1);
    }

    #[test]
    fn text_marks_dirty_and_unknown_without_claiming_clean() {
        let text = super::text();
        match super::source_dirty() {
            Some(true) => assert!(text.contains(" (dirty)"), "{text}"),
            Some(false) => assert!(!text.contains("dirty"), "{text}"),
            None => assert!(text.contains(" (dirty unknown)"), "{text}"),
        }
    }
}
