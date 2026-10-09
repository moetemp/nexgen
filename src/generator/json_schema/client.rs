//! The language-neutral plan an HTTP caller emitter works from.
//!
//! An emitted caller needs, per operation, the target's type name for the input
//! and output models and the wire name it posts to. The planned IR holds those
//! in different places, so this module assembles them once and the Python and
//! Go emitters differ only in how they render them.

use std::collections::BTreeMap;

use crate::error::Result;
use crate::language::Language;
use crate::planning::{PlannedFamily, PlannedSpec};
use crate::spec::{ExternalTypeSpec, OperationSpec, ServiceSpec, TypeSpec};

/// One operation's input or output.
#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientModel {
    /// The emitted type name in the target language.
    pub(in crate::generator) type_name: String,
    /// The model's name as the JSON backend knows it, used to derive sibling
    /// identifiers such as a converter class.
    pub(in crate::generator) model_name: String,
}

#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientOperation {
    pub(in crate::generator) name: String,
    /// The operation name on the wire, which the documentation quotes.
    pub(in crate::generator) wire_name: String,
    /// The wire name as the path segment a caller posts to. A wire name may
    /// hold any character, so it is percent-encoded here once rather than by
    /// each target.
    pub(in crate::generator) path_segment: String,
    pub(in crate::generator) doc: Option<String>,
    pub(in crate::generator) deprecated: bool,
    pub(in crate::generator) input: Option<ClientModel>,
    pub(in crate::generator) output: Option<ClientModel>,
}

#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientService {
    pub(in crate::generator) name: String,
    pub(in crate::generator) wire_name: String,
    pub(in crate::generator) doc: Option<String>,
    pub(in crate::generator) deprecated: bool,
    pub(in crate::generator) operations: Vec<ClientOperation>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::generator) struct ClientPlan {
    pub(in crate::generator) services: Vec<ClientService>,
}

impl ClientPlan {
    pub(in crate::generator) fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

/// How a target names the services and operations it emits. The two
/// derivations differ per target (verbatim `x-<lang>-name`, then that target's
/// casing), so they stay with the target rather than here.
pub(in crate::generator) struct ClientNaming<'a> {
    pub(in crate::generator) service: &'a dyn Fn(&ServiceSpec<PlannedFamily>) -> String,
    pub(in crate::generator) operation: &'a dyn Fn(&OperationSpec<PlannedFamily>) -> String,
}

/// Assembles the plan for one module.
///
/// `resolved_names` maps a model's full name to the identifier the target
/// emits for it, which is the JSON backend's own resolution with
/// `x-<lang>-name` applied. A model missing from the map keeps its planned
/// name, matching what the backends fall back to.
pub(in crate::generator) fn build_client_plan(
    api_plan: &PlannedSpec,
    resolved_names: &BTreeMap<String, String>,
    language: Language,
    naming: &ClientNaming<'_>,
) -> Result<ClientPlan> {
    let client_model = |type_spec: Option<&TypeSpec<PlannedFamily>>| -> Option<ClientModel> {
        let Some(TypeSpec::External(ExternalTypeSpec::Json(json_type))) = type_spec else {
            return None;
        };
        Some(ClientModel {
            type_name: resolved_names
                .get(&json_type.full_name)
                .cloned()
                .unwrap_or_else(|| json_type.model_name.clone()),
            model_name: json_type.model_name.clone(),
        })
    };

    let services = api_plan
        .services
        .iter()
        .map(|service| ClientService {
            name: (naming.service)(service),
            wire_name: service.wire_name.clone(),
            doc: service.doc.for_language(language).map(str::to_string),
            deprecated: service.deprecated,
            operations: service
                .operations
                .iter()
                .map(|operation| ClientOperation {
                    name: (naming.operation)(operation),
                    wire_name: operation.wire_name.clone(),
                    path_segment: path_segment(&operation.wire_name),
                    doc: operation.doc.for_language(language).map(str::to_string),
                    deprecated: operation.deprecated,
                    input: client_model(operation.input.as_ref()),
                    output: client_model(operation.output.as_ref()),
                })
                .collect(),
        })
        .collect();
    Ok(ClientPlan { services })
}

/// Percent-encodes a wire name as one URL path segment (RFC 3986). Only the
/// unreserved characters pass through, so the result is also safe inside any
/// target's string literal and format string.
fn path_segment(wire_name: &str) -> String {
    let mut encoded = String::with_capacity(wire_name.len());
    for byte in wire_name.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::path_segment;

    #[test]
    fn encodes_a_wire_name_as_one_path_segment() {
        assert_eq!(path_segment("read"), "read");
        assert_eq!(path_segment("poll-messages_v1.~"), "poll-messages_v1.~");
        assert_eq!(path_segment("a/b c"), "a%2Fb%20c");
        assert_eq!(path_segment("{x}\"q\""), "%7Bx%7D%22q%22");
        assert_eq!(path_segment("é"), "%C3%A9");
    }
}
