//! The streaming extension keywords and the language-neutral facts a generated
//! caller reads from them.
//!
//! The keywords are declared here so the loader that validates them and the
//! emitters that honor them share one vocabulary. Nothing here holds
//! target-language policy.
//!
//! See `specs/json-schema/long-poll.md` and `specs/json-schema/cursor.md` for
//! the authoritative rules.

use std::collections::BTreeSet;

use serde_json::Value;

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
    fn writes_the_rules_into_the_token_documentation() {
        let doc = cursor_doc("StreamCursor", " ");
        assert!(doc.starts_with("StreamCursor is an opaque resume token"));
        assert!(doc.contains("never parsed"));
        assert!(doc.contains("strictly after the record it names"));
    }
}
