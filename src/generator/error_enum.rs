//! The exception classes of an error enum, which Python, TypeScript and .NET generate.

use std::collections::BTreeSet;

use heck::ToUpperCamelCase;

use crate::error::{Error, Result};
use crate::language::Language;
use crate::planning::{PlannedFamily, PlannedSpec};
use crate::spec::{
    EnumErrorSpec, EnumSpec, EnumValueSpec, FieldOrigin, RecordFieldSpec, RecordSpec, TypeDeclSpec,
};

/// An error enum, with the names of its classes in one language.
pub(crate) struct ErrorClasses<'a> {
    /// The service of the enum's interface, whose calls convert failures to these classes.
    pub(crate) service: String,
    pub(crate) enumeration: &'a EnumSpec<PlannedFamily>,
    pub(crate) error: &'a EnumErrorSpec<PlannedFamily>,
    /// The SDK class that the base class extends.
    pub(crate) extends: &'a str,
    pub(crate) extends_import: Option<&'a str>,
    pub(crate) details: &'a RecordSpec<PlannedFamily>,
    /// The details field that holds the case.
    pub(crate) field: &'a RecordFieldSpec<PlannedFamily>,
    pub(crate) unspecified: &'a EnumValueSpec,
    pub(crate) base: ErrorClass<'a>,
    pub(crate) cases: Vec<ErrorClass<'a>>,
}

/// A class of an error enum.
pub(crate) struct ErrorClass<'a> {
    pub(crate) name: String,
    /// The failure type, which is the same in each language.
    pub(crate) failure_type: String,
    pub(crate) value: &'a EnumValueSpec,
    pub(crate) retryable: bool,
}

/// The error enums of a plan, for a language whose class names end in `suffix` in place of the
/// trailing `-error` of the enum's `name`. A language with no `<language>-extends` gets none,
/// and so does an enum whose details record or field the plan replaces or omits.
pub(crate) fn error_classes<'a>(
    api_plan: &'a PlannedSpec,
    language: Language,
    suffix: &str,
) -> Result<Vec<ErrorClasses<'a>>> {
    let mut classes = Vec::new();
    for entry in api_plan.types.values() {
        let TypeDeclSpec::Enum(enumeration) = &entry.declaration else {
            continue;
        };
        let Some(error) = &enumeration.error else {
            continue;
        };
        let Some(extends) = error.extends.value() else {
            continue;
        };
        let Some(details) = api_plan.record(&error.details) else {
            continue;
        };
        let Some(field) = details
            .fields
            .get(&error.field)
            .filter(|field| !matches!(field.origin, Some(FieldOrigin::Omitted)))
        else {
            continue;
        };
        let Some(unspecified) = enumeration.values.iter().find(|value| value.number == 0) else {
            continue;
        };
        let (interface, wit_name) = enumeration
            .full_name
            .rsplit_once('.')
            .unwrap_or(("", &enumeration.full_name));
        let stem = error
            .name
            .strip_suffix("-error")
            .unwrap_or(&error.name)
            .to_upper_camel_case();
        let base = ErrorClass {
            name: format!("{stem}{suffix}"),
            failure_type: format!("{stem}Error"),
            value: unspecified,
            retryable: false,
        };
        let mut names = BTreeSet::from([base.name.clone()]);
        let mut cases = Vec::new();
        for case in &error.cases {
            let Some(value) = enumeration
                .values
                .iter()
                .find(|value| value.wire_name == case.value)
            else {
                continue;
            };
            let name = case.name.value().map(str::to_string).unwrap_or_else(|| {
                let part = case
                    .value
                    .strip_prefix(&format!("{wit_name}-"))
                    .unwrap_or(&case.value);
                format!("{stem}{}{suffix}", part.to_upper_camel_case())
            });
            if !names.insert(name.clone()) {
                return Err(Error::ErrorClassNameConflict {
                    enumeration: enumeration.full_name.clone(),
                    name,
                    language,
                });
            }
            cases.push(ErrorClass {
                name,
                failure_type: case.failure_type.clone(),
                value,
                retryable: case.retryable,
            });
        }
        classes.push(ErrorClasses {
            service: interface.to_upper_camel_case(),
            enumeration,
            error,
            extends,
            extends_import: error.extends.import(),
            details,
            field,
            unspecified,
            base,
            cases,
        });
    }
    Ok(classes)
}
