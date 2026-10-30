//! Protobuf integer representation and compatibility for TypeScript resource bindings.

use std::collections::{BTreeMap, BTreeSet};

use super::integer_annotation;
use crate::error::{Error, Result};
use crate::generator::typescript::{
    RenderedResource, RenderedService, render_resource_with_annotations,
    typescript_resource_field_annotation, typescript_resource_requirements,
};
use crate::planning::{
    PlannedFamily, PlannedResource, PlannedResourceMethodBindingSpec, PlannedSpec, PlannedType,
    RequestPlan, RequestPlanSource, ResolvedResourceBindingSource, operation_input_model,
};
use crate::spec::{ExternalTypeSourceSpec, ExternalTypeSpec, IntSpec, ServiceSpec};

/// Render before returning to shared orchestration: no binding state escapes this backend.
pub(super) fn render_resource(
    api: &PlannedSpec,
    service: &ServiceSpec<PlannedFamily>,
    resource: &PlannedResource,
    rendered_service: &RenderedService<'_>,
) -> Result<RenderedResource> {
    let bindings = ResourceBindings::resolve(api, service, resource)?;
    let mut requirements = typescript_resource_requirements(resource);
    requirements.long |= bindings.requires_long(resource);
    let mut source = String::new();
    render_resource_with_annotations(
        &mut source,
        resource,
        rendered_service,
        |field| {
            typescript_resource_field_annotation(
                &field.kind,
                field.optional,
                field.function.as_ref(),
                bindings.field_is_proto(&field.name),
            )
        },
        |method, param| {
            typescript_resource_field_annotation(
                &param.kind,
                false,
                param.function.as_ref(),
                bindings.param_is_proto(&method.name, &param.name),
            )
        },
    );
    Ok(RenderedResource {
        source,
        requirements,
    })
}

/// Integer representation is a property of a binding, not of resource syntax.
/// Keep protobuf-specific facts private to the protobuf backend.
#[derive(Debug)]
struct ResourceBindings {
    proto_fields: BTreeSet<String>,
    proto_params: BTreeSet<(String, String)>,
}

impl ResourceBindings {
    fn field_is_proto(&self, name: &str) -> bool {
        self.proto_fields.contains(name)
    }

    fn param_is_proto(&self, method: &str, name: &str) -> bool {
        self.proto_params
            .contains(&(method.to_string(), name.to_string()))
    }

    fn requires_long(&self, resource: &PlannedResource) -> bool {
        resource.fields.iter().any(|field| {
            self.field_is_proto(&field.name)
                && field.function.is_none()
                && has_inline_64_bit_int(&field.kind)
        }) || resource.methods.iter().any(|method| {
            method.params.iter().any(|param| {
                self.param_is_proto(&method.name, &param.name)
                    && param.function.is_none()
                    && has_inline_64_bit_int(&param.kind)
            })
        })
    }

    fn resolve(
        api: &PlannedSpec,
        service: &ServiceSpec<PlannedFamily>,
        resource: &PlannedResource,
    ) -> Result<Self> {
        // Remember both the representation and its origin so a shared constructor field
        // cannot silently acquire incompatible types from different operations.
        let mut fields = BTreeMap::<String, (bool, String)>::new();
        let mut params = BTreeMap::new();
        let mut constrain = |name: &str, proto: bool, origin: String| -> Result<()> {
            let Some(field) = resource.fields.iter().find(|field| field.name == name) else {
                return Ok(());
            };
            if field.function.is_some() {
                return Ok(());
            }
            if let Some((previous, previous_origin)) = fields.get(name)
                && *previous != proto
                && has_inline_64_bit_int(&field.kind)
            {
                return Err(Error::InvalidResource {
                    service: service.name.clone(),
                    resource: resource.type_name.clone(),
                    reason: format!(
                        "64-bit field `{name}` has incompatible bindings: {previous_origin} \
                         requires `{}`, but {origin} requires `{}`",
                        if *previous {
                            integer_annotation(IntSpec::I64)
                        } else {
                            "number"
                        },
                        if proto {
                            integer_annotation(IntSpec::I64)
                        } else {
                            "number"
                        },
                    ),
                });
            }
            fields.insert(name.to_string(), (proto, origin));
            Ok(())
        };
        for method in &resource.methods {
            if let PlannedResourceMethodBindingSpec::Operation { request_plan, .. } =
                &method.binding
            {
                visit_request_sources(api, request_plan, false, &mut |source, proto| {
                    match source {
                        RequestPlanSource::ResourceField(name) => {
                            constrain(name, proto, format!("method `{}`", method.name))?;
                        }
                        RequestPlanSource::MethodParam(name) => {
                            let key = (method.name.clone(), name.clone());
                            if let Some(previous) = params.insert(key, proto)
                                && previous != proto
                                && method.params.iter().any(|param| {
                                    param.name == *name
                                        && param.function.is_none()
                                        && has_inline_64_bit_int(&param.kind)
                                })
                            {
                                return Err(Error::InvalidResourceMethod {
                                    service: service.name.clone(),
                                    resource: resource.type_name.clone(),
                                    method: method.name.clone(),
                                    reason: format!(
                                        "64-bit parameter `{name}` binds to both protobuf (`Long`) \
                                         and JSON (`number`) fields"
                                    ),
                                });
                            }
                        }
                    }
                    Ok(())
                })?;
            }
        }
        for operation in &service.operations {
            if let Some(returned) = &operation.data.output_resource_return
                && returned.resource_type_name == resource.type_name
            {
                for binding in &returned.bindings {
                    let kind = match &binding.source {
                        ResolvedResourceBindingSource::RequestField { .. } => {
                            operation_input_model(operation)
                        }
                        ResolvedResourceBindingSource::ResultField { .. } => {
                            operation.output_type()
                        }
                    };
                    constrain(
                        &binding.field_name,
                        kind.is_some_and(|kind| is_proto_model(api, kind)),
                        format!("operation `{}`", operation.name),
                    )?;
                }
            } else if let Some(PlannedType::Resource(output)) = operation.output_type()
                && output.type_name == resource.type_name
                && output.wire_type.is_none()
            {
                for field in &resource.fields {
                    constrain(
                        &field.name,
                        false,
                        format!("JSON result of `{}`", operation.name),
                    )?;
                }
            }
        }
        Ok(Self {
            proto_fields: fields
                .into_iter()
                .filter_map(|(name, (proto, _))| proto.then_some(name))
                .collect(),
            proto_params: params
                .into_iter()
                .filter_map(|(key, proto)| proto.then_some(key))
                .collect(),
        })
    }
}

fn has_inline_64_bit_int(kind: &PlannedType) -> bool {
    match kind {
        PlannedType::Int(IntSpec::I64 | IntSpec::U64) => true,
        PlannedType::Option(inner) | PlannedType::List(inner) => has_inline_64_bit_int(inner),
        // Object keys are strings, including protobuf's 64-bit map keys.
        PlannedType::Map(_, value) => has_inline_64_bit_int(value),
        PlannedType::Tuple(items) => items.iter().any(has_inline_64_bit_int),
        PlannedType::Result { ok, err } => {
            ok.as_deref().is_some_and(has_inline_64_bit_int)
                || err.as_deref().is_some_and(has_inline_64_bit_int)
        }
        PlannedType::External(ExternalTypeSpec::Alias(alias)) => {
            alias.type_name.value().is_none() && has_inline_64_bit_int(&alias.target)
        }
        // Named records own their field representations; do not inherit the parent's.
        _ => false,
    }
}

fn is_proto_model(api: &PlannedSpec, kind: &PlannedType) -> bool {
    match kind {
        PlannedType::Record(record) => api
            .record(&record.full_name)
            .is_some_and(|record| matches!(record.source, Some(ExternalTypeSourceSpec::Proto(_)))),
        PlannedType::External(ExternalTypeSpec::Proto(_)) => true,
        _ => false,
    }
}

fn visit_request_sources(
    api: &PlannedSpec,
    plan: &RequestPlan,
    proto: bool,
    visit: &mut impl FnMut(&RequestPlanSource, bool) -> Result<()>,
) -> Result<()> {
    match plan {
        RequestPlan::Source(source) => visit(source, proto),
        RequestPlan::Construct {
            message_name,
            fields,
        } => {
            // Construct names are either authored record names or descriptor message names.
            // The latter need not have a native declaration (e.g. external replacements).
            let proto = api.record(message_name).map_or(true, |record| {
                matches!(record.source, Some(ExternalTypeSourceSpec::Proto(_)))
            });
            for field in fields {
                visit_request_sources(api, &field.value, proto, visit)?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_integer_representation_depends_on_width() {
        for int in [IntSpec::I64, IntSpec::U64] {
            assert_eq!(integer_annotation(int), "Long");
        }
        for int in [IntSpec::I32, IntSpec::U32] {
            assert_eq!(integer_annotation(int), "number");
        }
    }
}
