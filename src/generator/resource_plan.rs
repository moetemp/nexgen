//! Shared rendering of resolved resource-method request plans.

use crate::planning::{RequestPlan, RequestPlanSource};

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
    FConstruct: Fn(&str, Vec<String>) -> String + Copy,
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
            render_construct(message_name, rendered_fields)
        }
    }
}

/// Makes each `return await <call>;` of a method body keep the response in `response`, and
/// return `value` of it instead. A call may span lines, up to the `;` at its own indent.
pub(crate) fn return_response_field(body: &str, declaration: &str, value: &str) -> String {
    let mut output = String::new();
    let mut open_indent = None::<String>;
    for line in body.lines() {
        let indent = &line[..line.len() - line.trim_start().len()];
        let line = match line.trim_start().strip_prefix("return await ") {
            Some(call) if open_indent.is_none() => {
                open_indent = Some(indent.to_string());
                format!("{indent}{declaration}{call}")
            }
            _ => line.to_string(),
        };
        output.push_str(&line);
        output.push('\n');
        if let Some(open) = &open_indent
            && line.ends_with(';')
            && (line.starts_with(&format!("{open}{declaration}")) || indent == open)
        {
            output.push_str(&format!("{open}return {value};\n"));
            open_indent = None;
        }
    }
    output
}
