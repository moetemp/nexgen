//! The streaming extension keywords and the language-neutral facts a generated
//! caller reads from them.
//!
//! The keywords are declared here so the loader that validates them and the
//! emitters that honor them share one vocabulary. Nothing here holds
//! target-language policy.
//!
//! See `specs/json-schema/long-poll.md`, `specs/json-schema/cursor.md`,
//! `specs/json-schema/stream-ref.md` and `specs/json-schema/handles.md` for the
//! authoritative rules.

use std::collections::BTreeSet;

use serde_json::Value;

/// Maps emitted handle type names to the ordered wire members they bind, on a
/// `services:` entry. An operation joins a handle when its input carries every
/// key member; the generated caller then exposes it as a method with those
/// members bound.
pub const HANDLE_KEYWORD: &str = "x-nexus-handle";

/// The base a handle constructor's name is derived from, before a target
/// cases it.
///
/// A trailing `Handle` is dropped, so `StreamHandle` is built by `stream`.
/// Built from a parent handle, the parent's own base is dropped as a prefix
/// when something is left of it, so `StreamProducer` built from `StreamHandle`
/// reads `producer`, while from the flat client it stays `stream_producer`.
/// One rule for every target keeps the two callers reading alike.
pub fn handle_constructor_base(name: &str, parent: Option<&str>) -> String {
    let base = name
        .strip_suffix("Handle")
        .filter(|rest| !rest.is_empty())
        .unwrap_or(name);
    if let Some(parent) = parent {
        let parent_base = handle_constructor_base(parent, None);
        if let Some(rest) = base.strip_prefix(parent_base.as_str())
            && rest.chars().next().is_some_and(char::is_uppercase)
        {
            return rest.to_string();
        }
    }
    base.to_string()
}

/// Marks a `$defs` model as the stream reference: the value an operation takes
/// or returns to hand a stream to its caller. The model's members are the wire
/// form. The value is `true`, or a map from a target's `x-<lang>-name` token to
/// the type that target emits in the model's place.
pub const STREAM_REF_KEYWORD: &str = "x-nexus-stream-ref";

/// The targets a stream reference can be mapped for, by their `x-<lang>-name`
/// token.
pub const STREAM_REF_MAPPED_TARGETS: [&str; 1] = ["py"];

/// Whether a schema node declares itself the stream reference.
pub fn stream_ref_marked(schema: &Value) -> bool {
    match schema.get(STREAM_REF_KEYWORD) {
        Some(Value::Bool(marked)) => *marked,
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

/// The type a stream reference maps to for one target, as a dotted
/// `module.Type` path, when the contract supplies one.
pub fn stream_ref_mapping<'a>(schema: &'a Value, target: &str) -> Option<&'a str> {
    schema.get(STREAM_REF_KEYWORD)?.get(target)?.as_str()
}

/// Marks a `type: string` member as an opaque resume token. The value names the
/// emitted token type, so every member naming the same type shares it.
pub const CURSOR_KEYWORD: &str = "x-nexus-cursor";

/// Marks an `operations:` entry as long-pollable.
pub const LONG_POLL_KEYWORD: &str = "x-nexus-long-poll";

/// The `x-nexus-long-poll` member naming the input's wait-hint member.
pub const LONG_POLL_WAIT_MEMBER: &str = "wait-field";

/// The `x-nexus-long-poll` member naming the output's emptiness-test member.
pub const LONG_POLL_RESULT_MEMBER: &str = "result-field";

/// The two rules a caller must not break. They are generator-owned rather than
/// copied from the authored `description`: a contract that words them
/// differently would still have to mean this.
const CURSOR_RULES: [&str; 2] = [
    "The value is never parsed, compared or constructed by a caller; only the endpoint that issued it can interpret it.",
    "Pass one back to resume strictly after the record it names, so that record is never delivered twice.",
];

/// The documentation an emitted token type carries.
///
/// The lead sentence names the type so it satisfies Go's godoc convention and
/// still reads correctly as a Python docstring. `paragraph_separator` is the
/// target's paragraph break: a space for a target whose doc comment is one
/// wrapped paragraph, `"\n\n"` for one that keeps paragraphs.
pub fn cursor_doc(name: &str, paragraph_separator: &str) -> String {
    format!(
        "{name} is an opaque resume token issued by the endpoint.{paragraph_separator}{}{paragraph_separator}{}",
        CURSOR_RULES[0], CURSOR_RULES[1]
    )
}

/// The token type a schema node declares itself a cursor for.
pub fn cursor_name(schema: &Value) -> Option<&str> {
    schema.get(CURSOR_KEYWORD)?.as_str()
}

/// Collects every token type declared in `schemas`, once each, in name order.
///
/// The walk is over raw authored schema, so a model no operation references
/// still contributes its type: the type is part of the contract's vocabulary,
/// not of one operation's signature.
pub fn cursor_type_names<'a>(schemas: impl IntoIterator<Item = &'a Value>) -> Vec<String> {
    let mut names = BTreeSet::new();
    for schema in schemas {
        collect_cursor_names(schema, &mut names);
    }
    names.into_iter().collect()
}

fn collect_cursor_names(schema: &Value, names: &mut BTreeSet<String>) {
    if let Some(name) = cursor_name(schema) {
        names.insert(name.to_string());
    }
    match schema {
        Value::Object(members) => {
            for value in members.values() {
                collect_cursor_names(value, names);
            }
        }
        Value::Array(entries) => {
            for entry in entries {
                collect_cursor_names(entry, names);
            }
        }
        _ => {}
    }
}

/// The positions inside one model's schema where a cursor is declared off the
/// model's direct properties, as authored paths.
///
/// A cursor names a member a caller reads its resume position from, so it has
/// to be one: anywhere else (an array element, a union branch, a map value) it
/// declares a token type and marks nothing a caller can read. Inline objects
/// are hoisted into models of their own before this runs, so a nested object's
/// member is its own model's direct property.
pub fn misplaced_cursors(schema: &Value) -> Vec<String> {
    let mut found = Vec::new();
    if cursor_name(schema).is_some() {
        found.push("the model root".to_string());
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (member, property) in properties {
            collect_misplaced(property, &format!("properties.{member}"), false, &mut found);
        }
    }
    for (keyword, value) in schema.as_object().into_iter().flatten() {
        if keyword != "properties" {
            collect_misplaced(value, keyword, true, &mut found);
        }
    }
    found
}

fn collect_misplaced(schema: &Value, path: &str, misplaced: bool, found: &mut Vec<String>) {
    match schema {
        Value::Object(members) => {
            if misplaced && cursor_name(schema).is_some() {
                found.push(path.to_string());
            }
            for (keyword, value) in members {
                if keyword != CURSOR_KEYWORD {
                    collect_misplaced(value, &format!("{path}.{keyword}"), true, found);
                }
            }
        }
        Value::Array(entries) => {
            for (index, entry) in entries.iter().enumerate() {
                collect_misplaced(entry, &format!("{path}[{index}]"), true, found);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collects_every_declared_token_type_once() {
        let schema = json!({
            "type": "object",
            "properties": {
                "token": { "type": "string", "x-nexus-cursor": "StreamCursor" },
                "other": { "type": "string", "x-nexus-cursor": "PageCursor" },
                "same": { "type": "string", "x-nexus-cursor": "StreamCursor" },
            },
        });
        assert_eq!(
            cursor_type_names([&schema]),
            vec!["PageCursor".to_string(), "StreamCursor".to_string()]
        );
    }

    #[test]
    fn accepts_a_cursor_on_a_direct_property() {
        let schema = json!({
            "type": "object",
            "properties": {
                "token": { "type": "string", "x-nexus-cursor": "StreamCursor" },
            },
        });
        assert!(misplaced_cursors(&schema).is_empty());
    }

    #[test]
    fn reports_a_cursor_off_a_direct_property() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tokens": {
                    "type": "array",
                    "items": { "type": "string", "x-nexus-cursor": "PageCursor" },
                },
            },
            "additionalProperties": { "type": "string", "x-nexus-cursor": "MapCursor" },
        });
        assert_eq!(
            misplaced_cursors(&schema),
            vec![
                "properties.tokens.items".to_string(),
                "additionalProperties".to_string(),
            ]
        );
    }

    #[test]
    fn derives_a_handle_constructor_from_its_name_and_parent() {
        assert_eq!(handle_constructor_base("StreamHandle", None), "Stream");
        assert_eq!(
            handle_constructor_base("StreamProducer", None),
            "StreamProducer"
        );
        assert_eq!(
            handle_constructor_base("StreamProducer", Some("StreamHandle")),
            "Producer"
        );
        // A parent whose base is not a prefix leaves the name whole.
        assert_eq!(
            handle_constructor_base("StreamProducer", Some("Session")),
            "StreamProducer"
        );
        // A name that is only the suffix stays itself rather than vanishing.
        assert_eq!(handle_constructor_base("Handle", None), "Handle");
        // The prefix has to end on a word boundary.
        assert_eq!(
            handle_constructor_base("Streamer", Some("StreamHandle")),
            "Streamer"
        );
    }

    #[test]
    fn reads_the_stream_ref_marker_off_a_model_root() {
        let schema = json!({
            "type": "object",
            "x-nexus-stream-ref": true,
            "properties": { "topic": { "type": "string" } },
            "additionalProperties": false,
        });
        assert!(stream_ref_marked(&schema));
        assert!(!stream_ref_marked(&json!({ "type": "object" })));
        assert!(!stream_ref_marked(&json!({ "x-nexus-stream-ref": false })));
        let mapped = json!({ "x-nexus-stream-ref": { "py": "sdk.streams.StreamRef" } });
        assert!(stream_ref_marked(&mapped));
        assert_eq!(
            stream_ref_mapping(&mapped, "py"),
            Some("sdk.streams.StreamRef")
        );
        assert_eq!(stream_ref_mapping(&schema, "py"), None);
    }

    #[test]
    fn writes_the_rules_into_the_token_documentation() {
        let doc = cursor_doc("StreamCursor", " ");
        assert!(doc.starts_with("StreamCursor is an opaque resume token"));
        assert!(doc.contains("never parsed"));
        assert!(doc.contains("strictly after the record it names"));
    }
}
