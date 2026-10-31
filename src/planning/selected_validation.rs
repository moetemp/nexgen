//! Validate the chosen generic surface after selection and before binding.
//! Authored descriptor checks remain in `AuthoredValidationPass`; this pass
//! consumes selected IR without cloning or selecting authored alternatives.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{Error, Result};
use crate::nexgen_config::GenerationTarget;
use crate::spec::{
    ApiSpec, ApiSpecLeaf, CompilerPass, ExternalTypeSpec, FieldFlattenSpec, FieldOrigin,
    FunctionArgsSpec, FunctionResultSpec, OperationSpec, RecordFieldSpec, RecordSpec,
    ResourceFlattenSpec, ResourceSpec, SelectedFamily, ServiceSpec, TypeSpec,
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
            validate_definition_sources(spec, service, operation, &invalid)?;
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
                     message to hold its type arguments",
                    operation.name, returned
                )));
            }
            validate_definition_parameter(spec, service, operation, returned.as_str(), &invalid)?;
        }
    }

    for (_, record) in spec.records() {
        for (field_name, field) in &record.fields {
            if let Some(flatten) = &field.flatten {
                validate_field_flatten(spec, record, field_name, field, flatten, &invalid)?;
            }
        }
        validate_promoted_names(spec, record, &invalid)?;
    }
    for resource in spec.services.iter().flat_map(|service| &service.resources) {
        if let Some(flatten) = &resource.flatten {
            validate_resource_flatten(spec, resource, flatten, &invalid)?;
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

/// A call of an operation that takes a definition names its parameter `definition`, next to the
/// fields of its request. A method that binds the operation passes runtime types.
fn validate_definition_parameter(
    spec: &ApiSpec<SelectedFamily>,
    service: &ServiceSpec<SelectedFamily>,
    operation: &OperationSpec<SelectedFamily>,
    returned: &str,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    if operation_takes_definition(service, operation, returned)
        && operation_request(spec, operation)
            .is_some_and(|request| request.fields.contains_key("definition"))
    {
        return Err(invalid(format!(
            "operation `{}` takes the definition of `{returned}` as `definition`, so its request \
             can't have a field named `definition`",
            operation.name
        )));
    }
    Ok(())
}

/// A source runs where the request is built. Only the Workflow call of an operation that takes a
/// definition has `definition` there.
fn validate_definition_sources(
    spec: &ApiSpec<SelectedFamily>,
    service: &ServiceSpec<SelectedFamily>,
    operation: &OperationSpec<SelectedFamily>,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let Some(request) = operation_request(spec, operation) else {
        return Ok(());
    };
    let takes_definition = match operation.output_type() {
        Some(TypeSpec::Resource(returned)) => {
            operation_takes_definition(service, operation, returned.as_str())
        }
        _ => false,
    };
    for (field_name, field) in &request.fields {
        let reads_definition = |target| {
            field
                .source_for(target)
                .is_some_and(|source| source_reads_definition(source))
        };
        if reads_definition(GenerationTarget::Client) {
            return Err(invalid(format!(
                "record `{}` field `{field_name}` has a client source that reads `definition`; \
                 only a Workflow source can read it",
                request.full_name
            )));
        }
        if reads_definition(GenerationTarget::Workflow) && !takes_definition {
            return Err(invalid(format!(
                "record `{}` field `{field_name}` has a source that reads `definition`, but \
                 operation `{}` takes no definition",
                request.full_name, operation.name
            )));
        }
    }
    Ok(())
}

fn operation_takes_definition(
    service: &ServiceSpec<SelectedFamily>,
    operation: &OperationSpec<SelectedFamily>,
    returned: &str,
) -> bool {
    service
        .resource(returned)
        .is_some_and(|resource| resource.definition.value().is_some())
        && !service.resources.iter().any(|resource| {
            resource
                .methods
                .iter()
                .any(|method| method.operation_name.as_deref() == Some(operation.name.as_str()))
        })
}

fn operation_request<'a>(
    spec: &'a ApiSpec<SelectedFamily>,
    operation: &OperationSpec<SelectedFamily>,
) -> Option<&'a RecordSpec<SelectedFamily>> {
    match operation.input_type() {
        Some(TypeSpec::Record(record)) => spec.record(record.as_str()),
        Some(TypeSpec::External(ExternalTypeSpec::Proto(proto))) => {
            spec.record_for_proto(proto.as_str())
        }
        _ => None,
    }
}

/// The record whose fields a flatten promotes. A record that flattens all of its fields already
/// can't flatten some of them. A flatten in a request needs a proto-backed record.
fn flattened_record<'a>(
    spec: &'a ApiSpec<SelectedFamily>,
    field_type: &TypeSpec<SelectedFamily>,
    require_proto: bool,
    context: &str,
    invalid: &dyn Fn(String) -> Error,
) -> Result<&'a RecordSpec<SelectedFamily>> {
    let nested = match field_type.without_option() {
        TypeSpec::Record(record) => spec.record(record.as_str()),
        TypeSpec::External(ExternalTypeSpec::Proto(proto)) => spec.record_for_proto(proto.as_str()),
        _ => None,
    };
    let Some(nested) = nested.filter(|nested| !require_proto || nested.source.is_some()) else {
        let kind = if require_proto {
            "a proto-backed record"
        } else {
            "a record"
        };
        return Err(invalid(format!(
            "{context} flattens fields, but its type isn't {kind}"
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
    // The factory only builds the record, so the record needs no proto.
    let nested = flattened_record(spec, &param.field_type, false, &context, invalid)?;
    // The factory takes the definition, the other parameters, and the parameter itself with
    // `keep`. Each language changes the case and the separators of a name.
    let normalized = |name: &str| {
        name.chars()
            .filter(|character| !matches!(character, '-' | '_'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let mut parameters = resource
        .fields
        .iter()
        .filter(|field| flatten.keep || field.name != flatten.param)
        .map(|field| {
            (
                normalized(&field.name),
                format!("parameter `{}`", field.name),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if resource.definition.value().is_some() {
        if let Some(clash) = parameters.get(&normalized("definition")) {
            return Err(invalid(format!(
                "resource `{}` takes a definition, so its factory can't also take {clash}",
                resource.name
            )));
        }
        parameters.insert(normalized("definition"), "the definition".to_string());
    }
    let omitted = |field: &RecordFieldSpec<SelectedFamily>| {
        matches!(field.origin, Some(FieldOrigin::Omitted))
    };
    for key in &flatten.fields {
        let Some(promoted) = nested.fields.get(key).filter(|field| !omitted(field)) else {
            return Err(invalid(format!(
                "{context} promotes `{key}`, which `{}` omits",
                nested.full_name
            )));
        };
        if let Some(other) = parameters.get(&normalized(&promoted.name)) {
            return Err(invalid(format!(
                "{context} promotes `{key}`, whose name `{}` is also the name of {other}",
                promoted.name
            )));
        }
    }
    if let Some((key, _)) = nested
        .fields
        .iter()
        .find(|(key, field)| field.required && !omitted(field) && !flatten.fields.contains(key))
    {
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
    flatten: &FieldFlattenSpec<Option<FieldOrigin>>,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let context = format!("record `{}` field `{field_name}`", parent.full_name);
    let nested = flattened_record(spec, &field.field_type, true, &context, invalid)?;
    if field.origin.is_some() {
        return Err(invalid(format!(
            "{context} flattens fields, so it can't have a source, a default or an api-omit too"
        )));
    }
    let omitted = |field: &RecordFieldSpec<SelectedFamily>| {
        matches!(field.origin, Some(FieldOrigin::Omitted))
    };
    let promoted = |key: &str| nested.fields.get(key).filter(|field| !omitted(field));
    for key in &flatten.fields {
        if promoted(key).is_none() {
            return Err(invalid(format!(
                "{context} promotes `{key}`, which `{}` omits",
                nested.full_name
            )));
        }
    }
    for (inner, outer) in &flatten.fills {
        let Some(inner_field) = promoted(inner) else {
            return Err(invalid(format!(
                "{context} fills `{inner}`, which `{}` omits",
                nested.full_name
            )));
        };
        let Some(outer_field) = parent.fields.get(outer).filter(|field| !omitted(field)) else {
            return Err(invalid(format!(
                "{context} fills `{inner}` from `{outer}`, which `{}` omits",
                parent.full_name
            )));
        };
        if inner_field.field_type.without_option() != outer_field.field_type.without_option() {
            return Err(invalid(format!(
                "{context} fills `{inner}` from `{outer}`, whose type differs"
            )));
        }
        // The call sets a sibling from a parameter or a source. It leaves out an api-omitted one,
        // and builds a flattened one from its promoted fields.
        if outer_field.api_omitted_for(GenerationTarget::Workflow)
            || outer_field.api_omitted_for(GenerationTarget::Client)
            || outer_field.flatten.is_some()
            || outer_field.function.is_some()
            || parent.function_for_args_field(outer).is_some()
        {
            return Err(invalid(format!(
                "{context} fills `{inner}` from `{outer}`, which the call doesn't set as a value"
            )));
        }
        if inner_field.required && !outer_field.required {
            return Err(invalid(format!(
                "{context} fills the required `{inner}` from `{outer}`, which is optional"
            )));
        }
    }
    if let Some((key, _)) = nested.fields.iter().find(|(key, nested_field)| {
        nested_field.required
            && !omitted(nested_field)
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

/// The call takes each promoted field as a parameter next to the other fields, so no two of them
/// can have one name. Each language changes the case and the separators of a name.
fn validate_promoted_names(
    spec: &ApiSpec<SelectedFamily>,
    record: &RecordSpec<SelectedFamily>,
    invalid: &dyn Fn(String) -> Error,
) -> Result<()> {
    let normalized = |name: &str| {
        name.chars()
            .filter(|character| !matches!(character, '-' | '_'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let parameters = record
        .fields
        .iter()
        .filter(|(_, field)| {
            !matches!(field.origin, Some(FieldOrigin::Omitted)) && field.flatten.is_none()
        })
        .map(|(field_name, field)| (normalized(&field.name), field_name))
        .collect::<BTreeMap<_, _>>();
    let mut promoted_names = BTreeMap::<String, String>::new();
    for (field_name, field) in &record.fields {
        let Some(flatten) = &field.flatten else {
            continue;
        };
        let nested = match field.field_type.without_option() {
            TypeSpec::Record(nested) => spec.record(nested.as_str()),
            TypeSpec::External(ExternalTypeSpec::Proto(proto)) => {
                spec.record_for_proto(proto.as_str())
            }
            _ => None,
        };
        for key in &flatten.fields {
            let Some(promoted) = nested.and_then(|nested| nested.fields.get(key)) else {
                continue;
            };
            let name = normalized(&promoted.name);
            if let Some(sibling) = parameters.get(&name) {
                return Err(invalid(format!(
                    "record `{}` field `{field_name}` promotes `{key}`, whose name `{}` is also \
                     the name of field `{sibling}`",
                    record.full_name, promoted.name
                )));
            }
            let owner = format!("`{key}` of field `{field_name}`");
            if let Some(first) = promoted_names.insert(name, owner.clone()) {
                return Err(invalid(format!(
                    "record `{}` promotes {first} and {owner}, which have one parameter name",
                    record.full_name
                )));
            }
        }
    }
    Ok(())
}

fn source_reads_definition(source: &str) -> bool {
    source.split_once('(').is_some_and(|(_, args)| {
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
    // A returned generic resource takes its type arguments from the resource that returns it.
    let parameters = |value: &TypeSpec<SelectedFamily>| {
        let returned = match value.without_option() {
            TypeSpec::Resource(returned) => service
                .resource(returned.as_str())
                .map(|returned| returned.type_parameters.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        spec.type_parameters_through_protos(value)
            .into_iter()
            .map(|usage| usage.parameter)
            .chain(returned)
            .collect::<Vec<_>>()
    };
    let undeclared = |value: &TypeSpec<SelectedFamily>| {
        parameters(value)
            .into_iter()
            .find(|parameter| !declared.contains(parameter.full_name.as_str()))
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
    if let Some(parameter) = resource_types.iter().find_map(|value| undeclared(value)) {
        return Err(invalid(format!(
            "resource `{}` uses the type parameter `{}`, which it doesn't declare. Add \
             `@nexus.type-parameter \"<alias>\"` to the resource for it",
            resource.name, parameter.full_name
        )));
    }
    let used = resource_types
        .iter()
        .flat_map(|value| parameters(value))
        .map(|parameter| parameter.full_name)
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
        if let Some(parameter) = operation
            .input_type()
            .into_iter()
            .chain(operation.output_type())
            .find_map(undeclared)
        {
            return Err(invalid(format!(
                "resource-bound operation `{operation_name}` uses the type parameter `{}`, which \
                 resource `{}` doesn't declare",
                parameter.full_name, resource.name
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
