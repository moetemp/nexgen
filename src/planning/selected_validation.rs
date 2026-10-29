//! Validate the chosen generic surface after selection and before binding.
//! Authored descriptor checks remain in `AuthoredValidationPass`; this pass
//! consumes selected IR without cloning or selecting authored alternatives.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{Error, Result};
use crate::spec::{
    ApiSpec, ApiSpecLeaf, CompilerPass, ExternalTypeSpec, FieldFlattenSpec, FieldOrigin,
    FunctionArgsSpec, FunctionResultSpec, RecordFieldSpec, RecordSpec, ResourceFlattenSpec,
    ResourceSpec, SelectedFamily, ServiceSpec, TypeSpec,
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
    // Each operation of an interface converts the failures of its one error enum.
    let mut error_enums = BTreeMap::<&str, &str>::new();
    for entry in spec.types.values() {
        let crate::spec::TypeDeclSpec::Enum(enumeration) = &entry.declaration else {
            continue;
        };
        if enumeration.error.is_none() {
            continue;
        }
        let name = enumeration.full_name.as_str();
        let interface = name.rsplit_once('.').map_or("", |(interface, _)| interface);
        if let Some(first) = error_enums.insert(interface, name) {
            return Err(invalid(format!(
                "`{first}` and `{name}` both have `@nexus.error`, but an interface has one error \
                 enum"
            )));
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
/// factory builds the record from these fields only, so it must promote each required field.
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
    // The factory takes the other parameters, and the parameter itself with `keep`. Each language
    // changes the case and the separators of a name.
    let normalized = |name: &str| {
        name.chars()
            .filter(|character| !matches!(character, '-' | '_'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let parameters = resource
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
    flatten: &FieldFlattenSpec,
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
    for key in &flatten.fields {
        if nested.fields.get(key).is_none_or(omitted) {
            return Err(invalid(format!(
                "{context} promotes `{key}`, which `{}` omits",
                nested.full_name
            )));
        }
    }
    if let Some((key, _)) = nested.fields.iter().find(|(key, nested_field)| {
        nested_field.required && !omitted(nested_field) && !flatten.fields.contains(key)
    }) {
        return Err(invalid(format!(
            "{context} flattens fields of `{}`, but doesn't promote its required field `{key}`",
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
