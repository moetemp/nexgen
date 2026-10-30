//! Shared rendering of resolved resource-method request plans.

use crate::error::{Error, Result};
use crate::nexgen_config::GenerationTarget;
use crate::planning::{
    PlannedFamily, PlannedProtoType, PlannedResourceMethodBindingSpec, PlannedSpec, RequestPlan,
    RequestPlanField, RequestPlanSource,
};
use crate::spec::{ExternalTypeSourceSpec, FieldOrigin, RecordSpec};

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

/// The plan of one target. A field whose source applies to the target takes the source, so the
/// plan doesn't fill it from a resource field or a method parameter. Each language renders this
/// plan, so they all follow one rule.
pub(crate) fn request_plan_for_target(
    api_plan: &PlannedSpec,
    plan: &RequestPlan,
    target: GenerationTarget,
) -> RequestPlan {
    let RequestPlan::Construct {
        message_name,
        fields,
    } = plan
    else {
        return plan.clone();
    };
    let record = record_for_request_name(api_plan, message_name);
    let sourced = |name: &str| {
        record.is_some_and(|record| {
            record
                .fields
                .values()
                .any(|field| field.name == name && field.source_for(target).is_some())
        })
    };
    RequestPlan::Construct {
        message_name: message_name.clone(),
        fields: fields
            .iter()
            .filter(|field| !sourced(&field.field_name))
            .map(|field| RequestPlanField {
                field_name: field.field_name.clone(),
                value: request_plan_for_target(api_plan, &field.value, target),
            })
            .collect(),
    }
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
                Some(FieldOrigin::Source { .. }) => "sourced",
                Some(FieldOrigin::Default { .. }) => "defaulted",
                _ => continue,
            };
            if field.required
                && field.source_for(target).is_none()
                && field.default_for(target).is_none()
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
    use crate::nexgen_config::{GenerationTarget, GenerationTargets, NexgenConfig, scope};
    use crate::planning::{PlannedResourceMethodBindingSpec, PlannedSpec, RequestPlan};
    use crate::spec::{ApiSpecNode, ApiSpecTree, CompilerPass};

    fn planned(wit: &str) -> PlannedSpec {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let _scope = scope(NexgenConfig {
            targets: GenerationTargets::ALL,
            ..NexgenConfig::default()
        });
        let descriptors =
            DescriptorIndex::load(&root.join("advanced/samples/descriptors/temporal_api.bin"))
                .unwrap();
        let spec = crate::parser::parse_api_spec_from_wit_for_language_with_inputs(
            Language::Python,
            wit,
            PathBuf::from("inline.wit"),
            &[root.join("advanced/samples/inputs/deps")],
        )
        .unwrap();
        let tree = crate::planning::LanguageSelectionPass::new(Language::Python)
            .apply(ApiSpecTree::single(spec))
            .unwrap();
        let tree =
            crate::planning::ResourceResolutionPass::new(&descriptors, GenerationTargets::ALL)
                .apply(tree)
                .unwrap();
        let tree = crate::planning::OperationBindingPass::new()
            .apply(tree)
            .unwrap();
        let tree = crate::planning::OperationLoweringPass::new()
            .apply(tree)
            .unwrap();
        let type_planning = crate::planning::TypePlanningPass::new(&tree, &descriptors);
        let ApiSpecNode::Leaf(leaf) = type_planning.apply(tree).unwrap().root else {
            panic!("one WIT file plans one leaf");
        };
        leaf.spec
    }

    fn planned_fields(plan: &RequestPlan) -> Vec<&str> {
        let RequestPlan::Construct { fields, .. } = plan else {
            panic!("the request plan constructs a record");
        };
        fields
            .iter()
            .map(|field| field.field_name.as_str())
            .collect()
    }

    #[test]
    fn a_source_that_applies_to_the_target_wins_over_a_resource_field() {
        let spec = planned(
            r#"
package temporal:sources@1.0.0;

world system { export jobs; }

/// @nexus.endpoint "jobs"
interface jobs {
  resource job {
    constructor(job-id: string, namespace: string);
    restart: func();
  }

  record restart-request {
    job-id: string,
    /// @nexus.source targets="workflow" python="workflow_namespace()"
    namespace: string,
  }

  restart: func(request: restart-request);
}
"#,
        );
        let method = &spec.services[0].resources[0].data.methods[0];
        let PlannedResourceMethodBindingSpec::Operation { request_plan, .. } = &method.binding
        else {
            panic!("`restart` binds the `restart` operation");
        };
        let workflow =
            super::request_plan_for_target(&spec, request_plan, GenerationTarget::Workflow);
        let client = super::request_plan_for_target(&spec, request_plan, GenerationTarget::Client);
        assert_eq!(planned_fields(&workflow), ["job-id"]);
        assert_eq!(planned_fields(&client), ["job-id", "namespace"]);
    }
}
