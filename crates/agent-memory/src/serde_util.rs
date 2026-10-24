//! Serde helpers shared by both transports' request DTOs (MCP `UpdateArgs` and
//! REST `UpdateBody`), so the two can never disagree on patch semantics.

use serde::{Deserialize, Deserializer};

/// Deserialize a JSON-merge-patch member into `Option<Option<T>>`, keeping
/// "explicit `null`" distinct from "absent" (D-08; serde issue #1042).
///
/// Plain `Option<Option<T>>` cannot do this: serde maps a JSON `null` to the
/// OUTER `None`, silently turning "clear this field" into "leave it alone".
/// Use it together with `#[serde(default)]`:
///
/// ```ignore
/// #[serde(default, deserialize_with = "crate::serde_util::deserialize_explicit_null")]
/// pub ttl_secs: Option<Option<i64>>,
/// ```
///
/// - member absent → the field's `Default`, `None` (this function never runs);
/// - `null` → `Some(None)`;
/// - a value → `Some(Some(v))`.
pub(crate) fn deserialize_explicit_null<'de, D, T>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Deserialize)]
    struct Probe {
        #[serde(default, deserialize_with = "deserialize_explicit_null")]
        ttl_secs: Option<Option<i64>>,
        #[serde(default, deserialize_with = "deserialize_explicit_null")]
        tags: Option<Option<Vec<String>>>,
        #[serde(default, deserialize_with = "deserialize_explicit_null")]
        scope: Option<Option<String>>,
    }

    fn probe(json: &str) -> Probe {
        serde_json::from_str(json).expect("probe JSON deserializes")
    }

    #[test]
    fn an_absent_member_is_none() {
        let p = probe("{}");
        assert_eq!(p.ttl_secs, None);
        assert_eq!(p.tags, None);
        assert_eq!(p.scope, None);
    }

    #[test]
    fn an_explicit_null_is_some_none() {
        let p = probe(r#"{"ttl_secs": null, "tags": null, "scope": null}"#);
        assert_eq!(p.ttl_secs, Some(None));
        assert_eq!(p.tags, Some(None));
        assert_eq!(p.scope, Some(None));
    }

    #[test]
    fn a_value_is_some_some() {
        let p = probe(r#"{"ttl_secs": 60, "tags": ["a", "b"], "scope": "project-x"}"#);
        assert_eq!(p.ttl_secs, Some(Some(60)));
        assert_eq!(p.tags, Some(Some(vec!["a".to_string(), "b".to_string()])));
        assert_eq!(p.scope, Some(Some("project-x".to_string())));
    }

    #[test]
    fn a_wrong_typed_value_is_still_a_deserialize_error() {
        let err = serde_json::from_str::<Probe>(r#"{"ttl_secs": "soon"}"#)
            .expect_err("a string is not an i64");
        assert!(err.to_string().contains("invalid type"), "got: {err}");
    }

    #[test]
    fn it_works_through_from_value_too() {
        // rmcp hands tool arguments over as a serde_json::Value map.
        let p: Probe = serde_json::from_value(serde_json::json!({ "scope": null }))
            .expect("deserializes from a Value");
        assert_eq!(p.scope, Some(None));
        assert_eq!(p.ttl_secs, None);
    }
}
