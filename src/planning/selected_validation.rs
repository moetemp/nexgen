//! Validate the chosen generic surface after selection and before binding.
//! Authored descriptor checks remain in `AuthoredValidationPass`; this pass
//! consumes selected IR without cloning or selecting authored alternatives.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{Error, Result};
use crate::spec::{
    ApiSpec, ApiSpecLeaf, CompilerPass, ExternalTypeSpec, FieldFlattenSpec, FunctionArgsSpec,
    FunctionResultSpec, OperationSpec, RecordFieldSpec, RecordFieldVisibility, RecordSpec,
    ResourceFlattenSpec, ResourceSpec, SelectedFamily, ServiceSpec, TargetFieldRole, TypeSpec,
};

pub(crate) struct SelectedValidationPass;

impl CompilerPass<SelectedFamily, SelectedFamily> for SelectedValidationPass {
    type Error = Error;

    fn transform_leaf(
        &mut self,
        leaf: ApiSpecLeaf<SelectedFamily>,
    ) -> Result<ApiSpecLeaf<SelectedFamily>> {
        validate_generic_model_semantics(&leaf.spec, &leaf.source_path)?;
        Ok(leaf)
    }
}

fn validate_generic_model_semantics(spec: &ApiSpec<SelectedFamily>, path: &Path) -> Result<()> {
    let invalid = |reason: String| Error::InvalidWit {
        path: path.to_path_buf(),
        reason,
    };
    for (_, record) in spec.records() {
        let parameters = spec.record_type_parameters(&record.full_name);
        let mut generated_names = BTreeMap::<String, String>::new();
        for usage in &parameters {
            if let Some(previous) = generated_names.insert(
                usage.parameter.name.clone(),
                usage.parameter.full_name.clone(),
            ) && previous != usage.parameter.full_name
            {
                return Err(invalid(format!(
                    "type parameters `{previous}` and `{}` both generate the name `{}`",
                    usage.parameter.full_name, usage.parameter.name
                )));
            }
        }
        for (field_name, function) in record.functions() {
            let uses_parameter = match &function.result {
                FunctionResultSpec::Authored(result) => !spec.type_parameters(result).is_empty(),
                FunctionResultSpec::Annotation(_) => false,
            } || match &function.args {
                FunctionArgsSpec::Varargs { prefix, .. } | FunctionArgsSpec::Fixed(prefix) => {
                    prefix
                        .iter()
                        .any(|arg| !spec.type_parameters(&arg.field_type).is_empty())
                }
            } || function
                .alternate_type
                .as_ref()
                .is_some_and(|alternate| !spec.type_parameters(alternate).is_empty());
            if uses_parameter {
                return Err(invalid(format!(
                    "record `{}` field `{field_name}` uses a type parameter in function-signature metadata, which is not supported",
                    record.full_name
                )));
            }
        }
    }

    for (_, variant) in spec.variants() {
        let parameters = spec.variant_type_parameters(&variant.full_name);
        let mut generated_names = BTreeMap::<String, String>::new();
        for usage in parameters {
            if let Some(previous) = generated_names.insert(
                usage.parameter.name.clone(),
                usage.parameter.full_name.clone(),
            ) && previous != usage.parameter.full_name
            {
                return Err(invalid(format!(
                    "variant `{}` uses type parameters `{previous}` and `{}` that both generate the name `{}`",
                    variant.full_name, usage.parameter.full_name, usage.parameter.name
                )));
            }
        }
    }

    for service in &spec.services {
        for operation in &service.operations {
            let mut generated_names = BTreeMap::<String, String>::new();
            let operation_parameters = operation
                .input_type()
                .into_iter()
                .chain(operation.output_type())
                .flat_map(|value| spec.type_parameters(value));
            for usage in operation_parameters {
                if let Some(previous) = generated_names.insert(
                    usage.parameter.name.clone(),
                    usage.parameter.full_name.clone(),
                ) && previous != usage.parameter.full_name
                {
                    return Err(invalid(format!(
                        "operation `{}` uses type parameters `{previous}` and `{}` that both generate the name `{}`",
                        operation.name, usage.parameter.full_name, usage.parameter.name
                    )));
                }
            }
        }
        for resource in &service.resources {
            validate_resource_type_parameters(spec, service, resource, &invalid)?;
        }
        for operation in &service.operations {
            validate_definition_sources(spec, service, operation, &invalid)?;
        }
        for operation in &service.operations {
            let Some(TypeSpec::Resource(returned)) = operation.output_type() else {
                continue;
            };
            if returned.wire_type.is_none()
                && service
                    .resource(returned.as_str())
                    .is_some_and(|resource| !resource.type_parameters.is_empty())
            {
                return Err(invalid(format!(
                    "operation `{}` returns the generic resource `{}` with no `@nexus.proto` \
                     message, which can't carry its type arguments",
                    operation.name, returned
                )));
            }
        }
    }

    for (_, record) in spec.records() {
        for (field_name, field) in &record.fields {
            if let Some(flatten) = &field.flatten {
                validate_field_flatten(spec, record, field_name, field, flatten, &invalid)?;
            }
        }
    }
    for resource in spec.services.iter().flat_map(|service| &service.resources) {
        if let Some(flatten) = &resource.flatten {
            validate_resource_flatten(spec, resource, flatten, &invalid)?;
        }
    }
    // A whole operation input is the request, so it can't be one of its own fields.
    for service in &spec.services {
        for operation in &service.operations {
            if let Some(TypeSpec::Record(record)) = &operation.input
                && spec
                    .record(record.as_str())
                    .is_some_and(|record| record.unwrap)
            {
                return Err(invalid(format!(
                    "operation `{}` takes `{record}` as its whole input, which `@nexus.unwrap` \
                     unwraps",
                    operation.name
                )));
            }
        }
    }

    // A record that holds a resource builds it from the record's message, with no type argument.
    let generic_resources = spec
        .services
        .iter()
        .flat_map(|service| &service.resources)
        .filter(|resource| !resource.type_parameters.is_empty())
        .map(|resource| resource.name.as_str())
        .collect::<BTreeSet<_>>();
    for (_, record) in spec.records() {
        for (field_name, field) in &record.fields {
            if let TypeSpec::Resource(resource) = field.field_type.without_option()
                && generic_resources.contains(resource.as_str())
            {
                return Err(invalid(format!(
                    "record `{}` field `{field_name}` holds the generic resource `{resource}`, \
                     which is not supported",
                    record.full_name
                )));
            }
        }
    }
    Ok(())
}

/// The proto-backed record whose fields a flatten promotes. A record that flattens all of its
/// fields already can't flatten some of them.
fn flattened_proto_record<'a>(
    spec: &'a ApiSpec<SelectedFamily>,
    field_type: &TypeSpec<SelectedFamily>,
    context: &str,
    invalid: &dyn Fn(String) -> Error,
) -> Result<&'a RecordSpec<SelectedFamily>> {
    let nested = match field_type.without_option() {
        TypeSpec::Record(record) => spec.record(record.as_str()),
        TypeSpec::External(ExternalTypeSpec::Proto(proto)) => spec.record_for_proto(proto.as_str()),
        _ => None,
    };
    let Some(nested) = nested.filter(|nested| nested.source.is_some()) else {
        return Err(invalid(format!(
            "{context} flattens fields, but its type isn't a proto-backed record"
        )));
    };
    if nested.flatten_in_api {
        return Err(invalid(format!(
            "{context} flattens fields of `{}`, which flattens all of its fields already",
            nested.full_name
        )));
    }
    Ok(nested)
}

/// A resource's flatten promotes fields of one constructor parameter's record in the factory. The
/// factory has no fills, so it must promote every required field of that record.
fn validate_resource_flatten(
    spec: &ApiSpec<SelectedFamily>,
    resource: &ResourceSpec<SelectedFamily>,
    flatten: &ResourceFlattenSpec,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let context = format!("resource `{}` parameter `{}`", resource.name, flatten.param);
    let Some(param) = resource
        .fields
        .iter()
        .find(|field| field.name == flatten.param)
    else {
        return Err(invalid(format!(
            "{context} isn't a parameter of the constructor"
        )));
    };
    let nested = flattened_proto_record(spec, &param.field_type, &context, invalid)?;
    for key in &flatten.fields {
        let Some(promoted) = nested
            .fields
            .get(key)
            .filter(|field| field.visibility != RecordFieldVisibility::Omitted)
        else {
            return Err(invalid(format!(
                "{context} promotes `{key}`, which `{}` omits",
                nested.full_name
            )));
        };
        if let Some(other) = resource
            .fields
            .iter()
            .find(|field| field.name != flatten.param && field.name == promoted.name)
        {
            return Err(invalid(format!(
                "{context} promotes `{key}`, whose name `{}` is also the name of parameter `{}`",
                promoted.name, other.name
            )));
        }
    }
    if let Some((key, _)) = nested.fields.iter().find(|(key, field)| {
        field.required
            && field.visibility != RecordFieldVisibility::Omitted
            && !flatten.fields.contains(key)
    }) {
        return Err(invalid(format!(
            "{context} flattens fields of `{}`, but doesn't promote its required field `{key}`",
            nested.full_name
        )));
    }
    Ok(())
}

/// A field-level flatten promotes fields of a proto-backed record, and the call must be able to
/// give every required field of that record.
fn validate_field_flatten(
    spec: &ApiSpec<SelectedFamily>,
    parent: &RecordSpec<SelectedFamily>,
    field_name: &str,
    field: &RecordFieldSpec<SelectedFamily>,
    flatten: &FieldFlattenSpec,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let context = format!("record `{}` field `{field_name}`", parent.full_name);
    let nested = flattened_proto_record(spec, &field.field_type, &context, invalid)?;
    if field.api_hidden() {
        return Err(invalid(format!(
            "{context} flattens fields, so it can't have a source or an api-omit too"
        )));
    }
    let promoted = |key: &str| {
        nested
            .fields
            .get(key)
            .filter(|field| field.visibility != RecordFieldVisibility::Omitted)
    };
    for key in &flatten.fields {
        let Some(promoted_field) = promoted(key) else {
            return Err(invalid(format!(
                "{context} promotes `{key}`, which `{}` omits",
                nested.full_name
            )));
        };
        if let Some((sibling_name, _)) = parent.fields.iter().find(|(name, sibling)| {
            name.as_str() != field_name
                && sibling.visibility != RecordFieldVisibility::Omitted
                && sibling.name == promoted_field.name
        }) {
            return Err(invalid(format!(
                "{context} promotes `{key}`, whose name `{}` is also the name of field \
                 `{sibling_name}`",
                promoted_field.name
            )));
        }
    }
    for (inner, outer) in &flatten.fills {
        let outer_field = parent
            .fields
            .get(outer)
            .filter(|field| field.visibility != RecordFieldVisibility::Omitted);
        let (Some(inner_field), Some(outer_field)) = (promoted(inner), outer_field) else {
            return Err(invalid(format!(
                "{context} fills `{inner}` from `{outer}`, which isn't a field"
            )));
        };
        if inner_field.field_type.without_option() != outer_field.field_type.without_option() {
            return Err(invalid(format!(
                "{context} fills `{inner}` from `{outer}`, whose type differs"
            )));
        }
    }
    if let Some((key, _)) = nested.fields.iter().find(|(key, nested_field)| {
        nested_field.required
            && nested_field.visibility != RecordFieldVisibility::Omitted
            && !flatten.fields.contains(key)
            && !flatten.fills.iter().any(|(inner, _)| inner == *key)
    }) {
        return Err(invalid(format!(
            "{context} flattens fields of `{}`, but neither promotes nor fills its required \
             field `{key}`",
            nested.full_name
        )));
    }
    Ok(())
}

/// A source may read `definition`, the definition argument of a call that returns a resource with
/// `@nexus.definition`. Target code runs such a source, so the source must name its target.
fn validate_definition_sources(
    spec: &ApiSpec<SelectedFamily>,
    service: &ServiceSpec<SelectedFamily>,
    operation: &OperationSpec<SelectedFamily>,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let record = match operation.input_type() {
        Some(TypeSpec::Record(record)) => spec.record(record.as_str()),
        Some(TypeSpec::External(ExternalTypeSpec::Proto(proto))) => {
            spec.record_for_proto(proto.as_str())
        }
        _ => None,
    };
    let Some(record) = record else {
        return Ok(());
    };
    let has_definition = match operation.output_type() {
        Some(TypeSpec::Resource(returned)) => service
            .resource(returned.as_str())
            .is_some_and(|resource| resource.definition.value().is_some()),
        _ => false,
    };
    for (field_name, field) in &record.fields {
        let untargeted = match &field.visibility {
            RecordFieldVisibility::Sourced { source_expr } if field.target_roles.is_none() => {
                Some(source_expr)
            }
            _ => None,
        };
        let targeted = field.target_roles.iter().flat_map(|roles| {
            [&roles.workflow, &roles.client]
                .into_iter()
                .filter_map(|role| match role {
                    TargetFieldRole::Sourced { source_expr } => Some(source_expr),
                    _ => None,
                })
        });
        if untargeted.is_some_and(|source_expr| source_reads_definition(source_expr)) {
            return Err(invalid(format!(
                "record `{}` field `{field_name}` has a source with no target that reads \
                 `definition`. Only target code has the definition, so name the target",
                record.full_name
            )));
        }
        if !has_definition
            && targeted
                .into_iter()
                .any(|expr| source_reads_definition(expr))
        {
            return Err(invalid(format!(
                "record `{}` field `{field_name}` has a source that reads `definition`, but \
                 operation `{}` takes no definition",
                record.full_name, operation.name
            )));
        }
    }
    Ok(())
}

/// Whether the arguments of a source call name `definition`.
fn source_reads_definition(source_expr: &str) -> bool {
    source_expr.split_once('(').is_some_and(|(_, args)| {
        args.split(|character: char| !(character.is_alphanumeric() || character == '_'))
            .any(|word| word == "definition")
    })
}

/// A resource may use only the type parameters that it declares, and it must use each of them.
/// The operations that its methods bind may use only these too.
fn validate_resource_type_parameters(
    spec: &ApiSpec<SelectedFamily>,
    service: &ServiceSpec<SelectedFamily>,
    resource: &ResourceSpec<SelectedFamily>,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let declared = resource
        .type_parameters
        .iter()
        .map(|parameter| parameter.full_name.as_str())
        .collect::<BTreeSet<_>>();
    let undeclared = |value: &TypeSpec<SelectedFamily>| {
        spec.type_parameters_through_protos(value)
            .into_iter()
            .find(|usage| !declared.contains(usage.parameter.full_name.as_str()))
    };
    let resource_types = resource
        .fields
        .iter()
        .map(|field| &field.field_type)
        .chain(resource.methods.iter().flat_map(|method| {
            method
                .params
                .iter()
                .map(|param| &param.field_type)
                .chain(method.result.as_ref().map(|result| &result.result_type))
        }))
        .collect::<Vec<_>>();
    if let Some(usage) = resource_types.iter().find_map(|value| undeclared(value)) {
        return Err(invalid(format!(
            "resource `{}` uses the type parameter `{}`, which it doesn't declare. Add \
             `@nexus.type-parameter \"<alias>\"` to the resource for it",
            resource.name, usage.parameter.full_name
        )));
    }
    let used = resource_types
        .iter()
        .flat_map(|value| spec.type_parameters_through_protos(value))
        .map(|usage| usage.parameter.full_name)
        .collect::<BTreeSet<_>>();
    if let Some(unused) = resource
        .type_parameters
        .iter()
        .find(|parameter| !used.contains(&parameter.full_name))
    {
        return Err(invalid(format!(
            "resource `{}` declares the type parameter `{}`, which none of its fields, \
             parameters or results uses",
            resource.name, unused.full_name
        )));
    }

    for method in &resource.methods {
        let Some(operation_name) = &method.operation_name else {
            continue;
        };
        let Some(operation) = service.operation(operation_name) else {
            continue;
        };
        if let Some(usage) = operation
            .input_type()
            .into_iter()
            .chain(operation.output_type())
            .find_map(undeclared)
        {
            return Err(invalid(format!(
                "resource-bound operation `{operation_name}` uses the type parameter `{}`, which \
                 resource `{}` doesn't declare",
                usage.parameter.full_name, resource.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::descriptors::DescriptorIndex;
    use crate::language::Language;
    use crate::planning::{AuthoredValidationPass, LanguageSelectionPass};
    use crate::spec::ApiSpecTree;

    fn validate(language: Language, wit: &str) -> Result<()> {
        let spec = crate::parser::parse_api_spec_from_wit_for_language_with_inputs(
            language,
            wit,
            PathBuf::from("inline.wit"),
            &[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("advanced/samples/inputs/deps")],
        )?;
        let descriptors = DescriptorIndex::load_many(&[])?;
        let authored =
            AuthoredValidationPass::new(&descriptors, language).apply(ApiSpecTree::single(spec))?;
        let selected = LanguageSelectionPass::new(language)
            .apply(authored)
            .expect("language selection is infallible");
        SelectedValidationPass.apply(selected).map(|_| ())
    }

    const GENERIC_WIT: &str = r#"
package temporal:nexus@1.0.0;
world system { export generic-service; }
interface generic-service {
  use nexus:temporal-types/model@1.0.0.{placeholder};
  /// @nexus.type-parameter
  type context-t = placeholder;
  /// @nexus.type-parameter
  type output-t = placeholder;
  record inner { value: context-t, }
  record request { nested: inner, values: list<context-t>, }
  record response { context: context-t, output: output-t, }
  complete: func(request: request) -> response;
}
"#;

    #[test]
    fn accepts_generic_models() {
        validate(Language::Dotnet, GENERIC_WIT).unwrap();
    }
}
