//! Shared rendering of resolved resource-method request plans, and the refs of resources.

use crate::error::{Error, Result};
use crate::planning::{
    PlannedFamily, PlannedProtoType, PlannedRecordType, PlannedResource, PlannedResourceField,
    PlannedResourceMethodBindingSpec, PlannedSpec, PlannedType, RequestPlan, RequestPlanField,
    RequestPlanSource,
};
use crate::spec::{ExternalTypeSourceSpec, FieldOrigin, RecordSpec, TypeParameterSpec};

/// A resource's ref is the constructor parameter that `@nexus.flatten-in-api` names.
pub(crate) fn resource_ref_field(resource: &PlannedResource) -> Option<&PlannedResourceField> {
    let name = resource.flatten.as_deref()?;
    resource.fields.iter().find(|field| field.name == name)
}

pub(crate) fn resource_ref_record(resource: &PlannedResource) -> Option<&PlannedRecordType> {
    match resource_ref_field(resource)?.kind.without_option() {
        PlannedType::Record(record) => Some(record),
        _ => None,
    }
}

/// The type parameters of each generic resource whose ref is the record, minus those of the
/// record's fields. Code passes a ref as data, so the ref needs the resource's type.
pub(crate) fn ref_type_parameters(
    api_plan: &PlannedSpec,
    record_full_name: &str,
) -> Vec<TypeParameterSpec> {
    let own = api_plan.record_type_parameters(record_full_name);
    let mut parameters = api_plan
        .services
        .iter()
        .flat_map(|service| service.resources.iter().map(|resource| &resource.data))
        .filter(|resource| {
            resource_ref_record(resource).is_some_and(|record| record.full_name == record_full_name)
        })
        .flat_map(|resource| resource.type_parameters.iter().cloned())
        .filter(|parameter| !own.iter().any(|usage| usage.parameter == *parameter))
        .collect::<Vec<_>>();
    parameters.sort_by(|left, right| {
        (left.declaration_index, &left.full_name).cmp(&(right.declaration_index, &right.full_name))
    });
    parameters.dedup();
    parameters
}

pub(crate) fn render_request_plan<FName, FAssign, FConstruct, FResource, FParam>(
    plan: &RequestPlan,
    member_name: FName,
    render_assignment: FAssign,
    render_construct: FConstruct,
    render_resource_field_source: FResource,
    render_method_param_source: FParam,
) -> String
where
    FName: Fn(&str) -> String + Copy,
    FAssign: Fn(String, String) -> String + Copy,
    FConstruct: Fn(&str, Vec<String>, &[RequestPlanField]) -> String + Copy,
    FResource: Fn(&str) -> String + Copy,
    FParam: Fn(&str) -> String + Copy,
{
    match plan {
        RequestPlan::Source(RequestPlanSource::ResourceField(name)) => {
            render_resource_field_source(name)
        }
        RequestPlan::Source(RequestPlanSource::MethodParam(name)) => {
            render_method_param_source(name)
        }
        RequestPlan::Construct {
            message_name,
            fields,
        } => {
            let rendered_fields = fields
                .iter()
                .map(|field| {
                    render_assignment(
                        member_name(&field.field_name),
                        render_request_plan(
                            &field.value,
                            member_name,
                            render_assignment,
                            render_construct,
                            render_resource_field_source,
                            render_method_param_source,
                        ),
                    )
                })
                .collect();
            render_construct(message_name, rendered_fields, fields)
        }
    }
}

/// The record of a request plan's message, by its WIT name or its proto message name.
pub(crate) fn record_for_request_name<'a>(
    api_plan: &'a PlannedSpec,
    name: &str,
) -> Option<&'a RecordSpec<PlannedFamily>> {
    api_plan.record(name).or_else(|| {
        let name = name.trim_start_matches('.');
        api_plan.records().map(|(_, record)| record).find(|record| {
            matches!(
                record.source.as_ref().and_then(ExternalTypeSourceSpec::proto_type),
                Some(PlannedProtoType::Message(message))
                    if message.proto.full_name == name
            )
        })
    })
}

/// Whether a request plan fills a top-level request field that has a source in `target`. The
/// resource's value wins over the source, so such a resource method must not call an operation
/// wrapper that fills the field from the source.
pub(crate) fn request_plan_fills_source(
    api_plan: &PlannedSpec,
    plan: &RequestPlan,
    target: crate::nexgen_config::GenerationTarget,
) -> bool {
    let RequestPlan::Construct {
        message_name,
        fields,
    } = plan
    else {
        return false;
    };
    let Some(record) = record_for_request_name(api_plan, message_name) else {
        return false;
    };
    fields.iter().any(|field| {
        record.fields.values().any(|candidate| {
            candidate.name == field.field_name && candidate.source_for(target).is_some()
        })
    })
}

/// Whether a resource method of the service binds the operation with a plan that fills one of
/// the operation's sourced fields.
pub(crate) fn resource_method_fills_source(
    api_plan: &PlannedSpec,
    service_name: &str,
    operation_name: &str,
    target: crate::nexgen_config::GenerationTarget,
) -> bool {
    api_plan
        .services
        .iter()
        .filter(|service| service.name == service_name)
        .flat_map(|service| service.resources.iter().map(|resource| &resource.data))
        .flat_map(|resource| &resource.methods)
        .any(|method| {
            matches!(
                &method.binding,
                PlannedResourceMethodBindingSpec::Operation {
                    operation_name: name,
                    request_plan,
                    ..
                } if name == operation_name
                    && request_plan_fills_source(api_plan, request_plan, target)
            )
        })
}

/// Check each API independently, preserving the shared request and source policy.
pub(crate) fn validate_resource_origins(
    api_plan: &PlannedSpec,
    target: crate::nexgen_config::GenerationTarget,
) -> Result<()> {
    for service in &api_plan.services {
        for resource in service.resources.iter().map(|resource| &resource.data) {
            for method in &resource.methods {
                let PlannedResourceMethodBindingSpec::Operation { request_plan, .. } =
                    &method.binding
                else {
                    continue;
                };
                if let Some((field, kind)) = missing_origin_field(api_plan, request_plan, target) {
                    return Err(Error::InvalidResourceMethod {
                        service: service.name.clone(),
                        resource: resource.type_name.clone(),
                        method: method.name.clone(),
                        reason: format!(
                            "the {kind} field `{field}` has no value for the `{}` target; \
                             supply it from a resource field or method parameter, or include this target in its origin",
                            match target {
                                crate::nexgen_config::GenerationTarget::Workflow => "workflow",
                                crate::nexgen_config::GenerationTarget::Client => "client",
                            },
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

fn missing_origin_field(
    api_plan: &PlannedSpec,
    plan: &RequestPlan,
    target: crate::nexgen_config::GenerationTarget,
) -> Option<(String, &'static str)> {
    let RequestPlan::Construct {
        message_name,
        fields,
        ..
    } = plan
    else {
        return None;
    };
    if let Some(record) = record_for_request_name(api_plan, message_name) {
        for field in record.fields.values() {
            let kind = match field.origin.as_ref() {
                Some(FieldOrigin::Default { .. }) => "defaulted",
                _ if field.has_source() => "sourced",
                _ => continue,
            };
            if field.required
                && field.source_for(target).is_none()
                && field.default_for(target).is_none()
                && !field.api_omitted_for(target)
                && fields
                    .iter()
                    .all(|planned| planned.field_name != field.name)
            {
                return Some((field.name.clone(), kind));
            }
        }
    }
    fields
        .iter()
        .find_map(|field| missing_origin_field(api_plan, &field.value, target))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::descriptors::DescriptorIndex;
    use crate::language::Language;
    use crate::planning::{
        OperationBindingPass, OperationLoweringPass, PlannedSpec, ResourceResolutionPass,
        ResourceTypeParameterPass, TypePlanningPass, select_spec,
    };
    use crate::spec::{ApiSpecLeaf, ApiSpecNode, ApiSpecTree, CompilerPass, ModulePath};

    fn plan(wit: &str) -> PlannedSpec {
        let authored = crate::parser::parse_api_spec_from_wit_for_language_with_inputs(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
            &[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("advanced/samples/inputs/deps")],
        )
        .unwrap();
        let descriptors = DescriptorIndex::load(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("advanced/samples/descriptors/temporal_api.bin"),
        )
        .unwrap();
        let tree = ApiSpecTree {
            root: ApiSpecNode::Leaf(ApiSpecLeaf {
                module_path: ModulePath::default(),
                source_root: PathBuf::new(),
                source_path: PathBuf::new(),
                spec: select_spec(authored, Language::Python),
            }),
        };
        let tree = ResourceTypeParameterPass.apply(tree).unwrap();
        let tree = ResourceResolutionPass::new(&descriptors, vec![])
            .apply(tree)
            .unwrap();
        let tree = OperationBindingPass::new().apply(tree).unwrap();
        let tree = OperationLoweringPass::new().apply(tree).unwrap();
        let tree = TypePlanningPass::new(&tree, &descriptors)
            .apply(tree)
            .unwrap();
        let ApiSpecNode::Leaf(leaf) = tree.root else {
            panic!("one leaf");
        };
        leaf.spec
    }

    #[test]
    fn only_the_ref_of_a_generic_resource_takes_its_type_parameters() {
        let plan = plan(
            r#"
package test:refs;

world system { export queues; }

/// @nexus.endpoint "queues"
interface queues {
  use nexus:temporal-types/model@1.0.0.{placeholder};

  /// @nexus.type-parameter
  type item-t = placeholder;

  record queue-ref { id: string }
  record push-request { ref: queue-ref, items: list<item-t> }
  record push-response { count: s64 }

  /// @nexus.flatten-in-api param="ref"
  resource queue {
    constructor(ref: queue-ref);
    push: func(items: list<item-t>) -> push-response;
  }

  push: func(request: push-request) -> push-response;

  // With no `@nexus.flatten-in-api`, the record isn't the resource's ref.
  record tag-ref { id: string }
  resource tagged {
    constructor(tag: tag-ref, label: item-t);
  }
}
"#,
        );
        let parameters = |name: &str| {
            let (_, record) = plan
                .records()
                .find(|(_, record)| record.name == name)
                .unwrap();
            super::ref_type_parameters(&plan, &record.full_name)
                .into_iter()
                .map(|parameter| parameter.name)
                .collect::<Vec<_>>()
        };

        assert_eq!(parameters("QueueRef"), ["ItemT"]);
        assert!(parameters("PushResponse").is_empty());
        assert!(parameters("TagRef").is_empty());
    }
}
