//! The language-neutral plan an HTTP caller emitter works from.
//!
//! An emitted caller needs, per operation, the target's type name for the input
//! and output models, the wire name it posts to, the long-poll member names, and
//! per service the handle projections with the target's type for each member.
//! The planned IR holds those in different places, so this module assembles
//! them once and the Python and Go emitters differ only in how they render them.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::json_schema::streaming;
use crate::language::Language;
use crate::planning::{PlannedFamily, PlannedSpec};
use crate::spec::{ExternalTypeSpec, OperationSpec, ServiceHandleSpec, ServiceSpec, TypeSpec};

/// One operation's input or output.
#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientModel {
    /// The emitted type name in the target language.
    pub(in crate::generator) type_name: String,
    /// The model's name as the JSON backend knows it, used to derive sibling
    /// identifiers such as a converter class.
    pub(in crate::generator) model_name: String,
    /// The model's authored schema, for the member lookups an emitter needs.
    pub(in crate::generator) schema: Value,
    /// The target's parameter type per top-level member, the way the models
    /// module types the field, so a handle method takes what the model holds.
    pub(in crate::generator) parameter_types: BTreeMap<String, String>,
}

impl ClientModel {
    /// The authored schema of one top-level member, when the model declares it.
    pub(in crate::generator) fn member(&self, wire_name: &str) -> Option<&Value> {
        self.schema.get("properties")?.get(wire_name)
    }

    /// The model's top-level members in authored order, as `(wire name, schema)`.
    pub(in crate::generator) fn members(&self) -> Vec<(&str, &Value)> {
        self.schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|properties| {
                properties
                    .iter()
                    .map(|(name, schema)| (name.as_str(), schema))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether a member admits one value only, so no caller chooses it.
    pub(in crate::generator) fn member_is_const(&self, wire_name: &str) -> bool {
        self.member(wire_name)
            .is_some_and(|member| member.get("const").is_some())
    }

    /// The target's parameter type for one member.
    pub(in crate::generator) fn member_type(&self, wire_name: &str) -> Result<String> {
        self.parameter_types
            .get(wire_name)
            .cloned()
            .ok_or_else(|| Error::InvalidJsonSchema {
                path: std::path::PathBuf::from("<client>"),
                reason: format!(
                    "model `{}` declares no member `{wire_name}`",
                    self.model_name
                ),
            })
    }

    /// Whether a handle parameter for the member takes a default: the member
    /// is optional on the wire, or the model fills in a schema default.
    pub(in crate::generator) fn member_takes_default(&self, wire_name: &str) -> bool {
        !self.member_required(wire_name)
            || self
                .member(wire_name)
                .is_some_and(|member| member.get("default").is_some())
    }

    /// Whether a top-level member is required, which decides whether a target
    /// holds it behind an optional.
    pub(in crate::generator) fn member_required(&self, wire_name: &str) -> bool {
        self.schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| required.iter().any(|name| name.as_str() == Some(wire_name)))
    }
}

/// The member names a long-poll loop drives.
#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientLongPoll {
    pub(in crate::generator) wait_member: String,
    pub(in crate::generator) result_member: String,
    /// The output's done flag, and whether the output requires it, which
    /// decides whether a target holds it behind an optional.
    pub(in crate::generator) done_member: Option<(String, bool)>,
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
    pub(in crate::generator) long_poll: Option<ClientLongPoll>,
}

/// One member a handle binds.
#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientHandleKey {
    pub(in crate::generator) wire_name: String,
    /// The member's authored schema, the same on every operation that joins.
    pub(in crate::generator) property: Value,
    pub(in crate::generator) required: bool,
    /// The target's type for the member as a constructor parameter and a
    /// stored field, optional when the member is. It is the first joining
    /// operation's; a target that names a type after its model converts it
    /// into each other operation's own.
    pub(in crate::generator) type_name: String,
}

/// A handle constructible from another by binding the keys it adds.
#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientHandleChild {
    /// Index into [`ClientService::handles`].
    pub(in crate::generator) handle: usize,
    /// The language-neutral base of the constructor's name on the parent.
    pub(in crate::generator) constructor_base: String,
    /// Indexes into the child's keys that the parent does not bind, in the
    /// child's key order.
    pub(in crate::generator) extra_keys: Vec<usize>,
}

/// One handle projection: the operations it carries and what it is built from.
#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientHandle {
    pub(in crate::generator) name: String,
    /// The language-neutral base of the constructor's name on the flat caller.
    pub(in crate::generator) constructor_base: String,
    pub(in crate::generator) keys: Vec<ClientHandleKey>,
    /// Indexes into [`ClientService::operations`] whose input carries every key.
    pub(in crate::generator) operations: Vec<usize>,
    pub(in crate::generator) children: Vec<ClientHandleChild>,
}

impl ClientHandle {
    pub(in crate::generator) fn binds(&self, wire_name: &str) -> bool {
        self.keys.iter().any(|key| key.wire_name == wire_name)
    }
}

/// How a target types one input member when a handle method takes it as a
/// parameter or stores it as a key: the models module's own rule, so the
/// handle hands the model exactly what its field holds. The arguments are the
/// model's emitted name, the member's wire name, its schema and whether it is
/// required; a target that names closed-value types after the model needs the
/// first two.
pub(in crate::generator) type MemberTyping<'a> =
    &'a dyn Fn(&str, &str, &Value, bool) -> Result<String>;

#[derive(Debug, Clone)]
pub(in crate::generator) struct ClientService {
    pub(in crate::generator) name: String,
    pub(in crate::generator) wire_name: String,
    pub(in crate::generator) doc: Option<String>,
    pub(in crate::generator) deprecated: bool,
    pub(in crate::generator) operations: Vec<ClientOperation>,
    pub(in crate::generator) handles: Vec<ClientHandle>,
}

#[derive(Debug, Clone, Default)]
pub(in crate::generator) struct ClientPlan {
    pub(in crate::generator) services: Vec<ClientService>,
}

impl ClientPlan {
    pub(in crate::generator) fn is_empty(&self) -> bool {
        self.services.is_empty()
    }

    /// Whether any operation declares a long-poll contract.
    pub(in crate::generator) fn has_long_poll(&self) -> bool {
        self.services
            .iter()
            .flat_map(|service| service.operations.iter())
            .any(|operation| operation.long_poll.is_some())
    }
}

/// How a target names the services and operations it emits. The two
/// derivations differ per target (verbatim `x-<lang>-name`, then that target's
/// casing), so they stay with the target rather than here.
pub(in crate::generator) struct ClientNaming<'a> {
    pub(in crate::generator) service: &'a dyn Fn(&ServiceSpec<PlannedFamily>) -> String,
    pub(in crate::generator) operation: &'a dyn Fn(&OperationSpec<PlannedFamily>) -> String,
    /// How the target types an input member a handle binds or takes.
    pub(in crate::generator) member: MemberTyping<'a>,
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
    let client_model =
        |type_spec: Option<&TypeSpec<PlannedFamily>>| -> Result<Option<ClientModel>> {
            let Some(TypeSpec::External(ExternalTypeSpec::Json(json_type))) = type_spec else {
                return Ok(None);
            };
            let required = json_type
                .schema
                .get("required")
                .and_then(Value::as_array)
                .map(|names| {
                    names
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<BTreeSet<_>>()
                })
                .unwrap_or_default();
            let type_name = resolved_names
                .get(&json_type.full_name)
                .cloned()
                .unwrap_or_else(|| json_type.model_name.clone());
            let mut parameter_types = BTreeMap::new();
            if let Some(properties) = json_type
                .schema
                .get("properties")
                .and_then(Value::as_object)
            {
                for (name, property) in properties {
                    parameter_types.insert(
                        name.clone(),
                        (naming.member)(
                            &type_name,
                            name,
                            property,
                            required.contains(name.as_str()),
                        )?,
                    );
                }
            }
            Ok(Some(ClientModel {
                type_name,
                model_name: json_type.model_name.clone(),
                schema: json_type.schema.clone(),
                parameter_types,
            }))
        };

    let mut services = Vec::new();
    for service in &api_plan.services {
        let mut operations = Vec::new();
        for operation in &service.operations {
            let output = client_model(operation.output.as_ref())?;
            let long_poll = operation
                .long_poll
                .as_ref()
                .map(|long_poll| ClientLongPoll {
                    wait_member: long_poll.wait_field.clone(),
                    result_member: long_poll.result_field.clone(),
                    done_member: long_poll.done_field.as_ref().map(|done| {
                        let required = output
                            .as_ref()
                            .is_some_and(|model| model.member_required(done));
                        (done.clone(), required)
                    }),
                });
            operations.push(ClientOperation {
                name: (naming.operation)(operation),
                wire_name: operation.wire_name.clone(),
                path_segment: path_segment(&operation.wire_name),
                doc: operation.doc.for_language(language).map(str::to_string),
                deprecated: operation.deprecated,
                input: client_model(operation.input.as_ref())?,
                output,
                long_poll,
            });
        }
        let handles = plan_handles(&service.handles, &operations)?;
        services.push(ClientService {
            name: (naming.service)(service),
            wire_name: service.wire_name.clone(),
            doc: service.doc.for_language(language).map(str::to_string),
            deprecated: service.deprecated,
            operations,
            handles,
        });
    }
    Ok(ClientPlan { services })
}

/// Projects the authored handles over the service's operations.
///
/// Membership and derivation both fall out of the key sets: an operation joins
/// every handle whose keys its input carries, and a handle is constructible
/// from every handle whose key set its own extends. The loader has already
/// refused a handle nothing joins and keys declared two ways, so the first
/// joining operation's declaration stands for all of them here.
fn plan_handles(
    declared: &[ServiceHandleSpec],
    operations: &[ClientOperation],
) -> Result<Vec<ClientHandle>> {
    let mut handles = Vec::new();
    for handle in declared {
        let joined = operations
            .iter()
            .enumerate()
            .filter(|(_, operation)| {
                operation
                    .input
                    .as_ref()
                    .is_some_and(|input| handle.keys.iter().all(|key| input.member(key).is_some()))
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let Some(first) = joined
            .first()
            .and_then(|index| operations[*index].input.as_ref())
        else {
            continue;
        };
        let mut keys = Vec::new();
        for key in &handle.keys {
            keys.push(ClientHandleKey {
                wire_name: key.clone(),
                property: first.member(key).cloned().unwrap_or(Value::Null),
                required: first.member_required(key),
                type_name: first.member_type(key)?,
            });
        }
        handles.push(ClientHandle {
            name: handle.name.clone(),
            constructor_base: streaming::handle_constructor_base(&handle.name, None),
            keys,
            operations: joined,
            children: Vec::new(),
        });
    }
    for parent_index in 0..handles.len() {
        let parent_keys = handles[parent_index]
            .keys
            .iter()
            .map(|key| key.wire_name.clone())
            .collect::<BTreeSet<_>>();
        let mut children = Vec::new();
        for (child_index, child) in handles.iter().enumerate() {
            let child_keys = child
                .keys
                .iter()
                .map(|key| key.wire_name.clone())
                .collect::<BTreeSet<_>>();
            if child_keys.len() <= parent_keys.len() || !parent_keys.is_subset(&child_keys) {
                continue;
            }
            children.push(ClientHandleChild {
                handle: child_index,
                constructor_base: streaming::handle_constructor_base(
                    &child.name,
                    Some(&handles[parent_index].name),
                ),
                extra_keys: child
                    .keys
                    .iter()
                    .enumerate()
                    .filter(|(_, key)| !parent_keys.contains(&key.wire_name))
                    .map(|(index, _)| index)
                    .collect(),
            });
        }
        handles[parent_index].children = children;
    }
    Ok(handles)
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
