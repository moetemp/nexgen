//! Validate the chosen generic surface after selection and before binding.
//! Authored descriptor checks remain in `AuthoredValidationPass`; this pass
//! consumes selected IR without cloning or selecting authored alternatives.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{Error, Result};
use crate::spec::{
    ApiSpec, ApiSpecLeaf, CompilerPass, ExternalTypeSpec, FieldFlattenSpec, FieldOrigin,
    FunctionArgsSpec, FunctionResultSpec, RecordFieldSpec, RecordSpec, SelectedFamily, TypeSpec,
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
    let nested = match field.field_type.without_option() {
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::descriptors::DescriptorIndex;
    use crate::language::Language;
    use crate::planning::{
        AuthoredValidationPass, LanguageSelectionPass, ResourceTypeParameterPass,
    };
    use crate::spec::{ApiSpecNode, ApiSpecTree};

    fn selected(language: Language, wit: &str) -> Result<ApiSpecTree<SelectedFamily>> {
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
        ResourceTypeParameterPass.apply(selected)
    }

    fn validate(language: Language, wit: &str) -> Result<()> {
        SelectedValidationPass
            .apply(selected(language, wit)?)
            .map(|_| ())
    }

    fn resource_type_parameters(wit: &str, resource: &str) -> Vec<String> {
        let tree = selected(Language::Python, wit).unwrap();
        let ApiSpecNode::Leaf(leaf) = tree.root else {
            panic!("expected one module");
        };
        leaf.spec
            .services
            .iter()
            .find_map(|service| service.resource(resource))
            .expect("the service has the resource")
            .type_parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect()
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

    // The resource has no annotation. It holds the parameter only through its records.
    const RESOURCE_WIT: &str = r#"
package temporal:nexus@1.0.0;
world system { export handles; }
/// @nexus.endpoint "handles"
interface handles {
  use nexus:temporal-types/model@1.0.0.{placeholder};
  /// @nexus.type-parameter
  type body-t = placeholder;
  /// @nexus.type-parameter
  type key-t = placeholder;
  record item { body: body-t, }
  record page { items: list<item>, }
  record put-request { id: string, item: item, }
  record put-response {}
  record get-request { id: string, }
  resource handle {
    constructor(id: string);
    /// @nexus.operation "put"
    put: func(item: item) -> put-response;
    /// @nexus.operation "get"
    get: func() -> page;
  }
  resource plain {
    constructor(id: string, key: string);
  }
  put: func(request: put-request) -> put-response;
  get: func(request: get-request) -> page;
}
"#;

    #[test]
    fn a_resource_is_generic_in_the_parameters_of_its_records() {
        assert_eq!(resource_type_parameters(RESOURCE_WIT, "handle"), ["BodyT"]);
        assert!(resource_type_parameters(RESOURCE_WIT, "plain").is_empty());
    }

    #[test]
    fn a_resource_takes_its_type_parameters_in_the_order_that_it_uses_them() {
        let wit = r#"
package test:pairs@1.0.0;
world system { export pairs; }
interface pairs {
  type placeholder = string;
  /// @nexus.type-parameter
  type key-t = placeholder;
  /// @nexus.type-parameter
  type value-t = placeholder;
  record entry { value: value-t, }
  resource pair {
    constructor(value: entry, key: key-t);
  }
}
"#;
        assert_eq!(resource_type_parameters(wit, "pair"), ["ValueT", "KeyT"]);
    }
}
