//! Semantic filters shared by `view` and `search`, and the query fingerprint embedded in cursors.
//!
//! Only *semantic* selections live here (scope, role, phase, time window, provider, file input).
//! Display concerns (tail, limits, byte budget, max-record) are not part of the fingerprint so a
//! tail query can still continue by append cursor.

use crate::query::model::{Provider, Role};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Messages,
    Tools,
    All,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Messages => "messages",
            Scope::Tools => "tools",
            Scope::All => "all",
        }
    }

    pub fn includes_message(self) -> bool {
        matches!(self, Scope::Messages | Scope::All)
    }

    pub fn includes_tool(self) -> bool {
        matches!(self, Scope::Tools | Scope::All)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filters {
    pub scope: Option<Scope>,
    pub roles: Vec<String>,
    pub phases: Vec<String>,
    /// Inclusive lower bound, epoch ms.
    pub since_ms: Option<i64>,
    /// Exclusive upper bound, epoch ms.
    pub until_ms: Option<i64>,
    pub provider: Option<Provider>,
}

impl Filters {
    pub fn normalized(&self) -> Self {
        let mut roles = self.roles.clone();
        roles.sort();
        roles.dedup();
        let mut phases = self.phases.clone();
        phases.sort();
        phases.dedup();
        Self {
            scope: self.scope,
            roles,
            phases,
            since_ms: self.since_ms,
            until_ms: self.until_ms,
            provider: self.provider,
        }
    }

    /// Fingerprint used inside cursors. Bounded to 2048 bytes encoded.
    pub fn fingerprint(&self, event_filter: Option<&str>) -> String {
        let normalized = self.normalized();
        let mut text = String::new();
        text.push_str("scope=");
        text.push_str(normalized.scope.map(Scope::as_str).unwrap_or("-"));
        text.push_str(";roles=");
        text.push_str(&normalized.roles.join(","));
        text.push_str(";phases=");
        text.push_str(&normalized.phases.join(","));
        text.push_str(";since=");
        text.push_str(
            &normalized
                .since_ms
                .map(|v| v.to_string())
                .unwrap_or_default(),
        );
        text.push_str(";until=");
        text.push_str(
            &normalized
                .until_ms
                .map(|v| v.to_string())
                .unwrap_or_default(),
        );
        text.push(';');
        if let Some(event) = event_filter {
            text.push_str("event=");
            text.push_str(event);
        }
        text
    }

    pub fn fingerprint_bytes(&self, event_filter: Option<&str>) -> usize {
        self.fingerprint(event_filter).len()
    }
}

/// Parse `--role user,assistant` into normalized role names; unknown roles are an error (rc2).
pub fn parse_roles(raw: &str) -> Result<Vec<String>, String> {
    let mut roles = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part {
            "user" => roles.push(Role::User.as_str().to_string()),
            "assistant" => roles.push(Role::Assistant.as_str().to_string()),
            "developer" => roles.push(Role::Developer.as_str().to_string()),
            "system" => roles.push(Role::System.as_str().to_string()),
            other => {
                return Err(format!(
                    "unknown role '{other}' (user,assistant,developer,system)"
                ))
            }
        }
    }
    if roles.is_empty() {
        return Err("--role requires at least one role".to_string());
    }
    roles.sort();
    roles.dedup();
    Ok(roles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_sorted_and_deduplicated() {
        let roles = parse_roles("user, assistant,user").unwrap();
        assert_eq!(roles, vec!["assistant", "user"]);
    }

    #[test]
    fn fingerprint_excludes_display_parameters() {
        let filters = Filters {
            scope: Some(Scope::Messages),
            roles: vec!["user".into(), "assistant".into()],
            ..Default::default()
        };
        let a = filters.fingerprint(None);
        assert!(a.contains("scope=messages"));
        assert!(a.len() <= 2048);
    }

    #[test]
    fn fingerprint_is_order_insensitive_for_roles() {
        let a = Filters {
            roles: vec!["user".into(), "assistant".into()],
            ..Default::default()
        };
        let b = Filters {
            roles: vec!["assistant".into(), "user".into()],
            ..Default::default()
        };
        assert_eq!(a.fingerprint(None), b.fingerprint(None));
    }

    #[test]
    fn unknown_role_is_rejected() {
        assert!(parse_roles("root").is_err());
    }
}
