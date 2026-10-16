//! The generated Python forwarding handler (`--forwarder`).
//!
//! A runtime that serves the contract itself, such as one in Core, needs only
//! the operation's input in its JSON transfer form and hands back the output
//! in the same form. The handler does that conversion per operation and holds
//! no protocol rule, so each SDK keeps only what the runtime can't do, such as
//! running user code before the call.

use crate::error::{Error, Result};
use crate::generator::json_schema::client::{ClientOperation, ClientPlan, ClientService};
use crate::generator::json_schema::python::converter_class_name;
use crate::generator::json_schema::python_client::render_docstring;
use crate::generator::python::render_generated_file_header;

/// The file the handler is emitted to, next to the models and services it uses.
pub(in crate::generator) const FORWARDER_MODULE: &str = "forwarder.py";

pub(in crate::generator) fn forwarder_class_name(service_name: &str) -> String {
    format!("{service_name}Forwarder")
}

fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("a Rust string is always JSON-serializable")
}

/// The members the class declares beside its operations.
const FORWARDER_MEMBERS: [&str; 1] = ["forward"];

/// Renders the whole module, or `None` when the module declares no service.
///
/// An operation that takes a member's name would replace that member, so it
/// is refused rather than emitted.
pub(in crate::generator) fn render_forwarder_module(plan: &ClientPlan) -> Result<Option<String>> {
    for service in &plan.services {
        for operation in &service.operations {
            if FORWARDER_MEMBERS.contains(&operation.name.as_str()) {
                return Err(Error::ForwarderMemberClash {
                    service: service.wire_name.clone(),
                    operation: operation.wire_name.clone(),
                    name: operation.name.clone(),
                });
            }
        }
    }
    if plan.is_empty() {
        return Ok(None);
    }
    let mut output = String::new();
    render_generated_file_header(&mut output);
    output.push('\n');
    output.push_str("import abc\n");
    output.push_str("import typing\n\n");
    output.push_str("import nexusrpc\n");
    output.push_str("import nexusrpc.handler\n\n");
    render_model_imports(&mut output, plan);
    for service in &plan.services {
        output.push_str("\n\n");
        render_forwarder_class(&mut output, service);
    }
    Ok(Some(output))
}

fn render_model_imports(output: &mut String, plan: &ClientPlan) {
    let mut names = std::collections::BTreeSet::new();
    for service in &plan.services {
        for operation in &service.operations {
            for model in [operation.input.as_ref(), operation.output.as_ref()]
                .into_iter()
                .flatten()
            {
                names.insert(model.type_name.clone());
                names.insert(converter_class_name(&model.model_name));
            }
        }
    }
    if names.is_empty() {
        return;
    }
    // The converters are the models module's own private classes, versioned
    // with this module, so reaching for them is a package-internal call.
    output.push_str("from .models import (\n");
    for name in &names {
        output.push_str("    ");
        output.push_str(name);
        output.push_str(",\n");
    }
    output.push_str(")\n");
}

fn render_forwarder_class(output: &mut String, service: &ClientService) {
    output.push_str("class ");
    output.push_str(&forwarder_class_name(&service.name));
    output.push_str("(abc.ABC):\n");
    let mut paragraphs = vec![
        format!(
            "Serves the {} service by forwarding each operation to :meth:`forward`.",
            service.wire_name
        ),
        "Each operation turns its input into the JSON transfer form, hands it to \
         ``forward`` with the operation's wire name, and turns the answer back into the \
         output model. The class holds no protocol rule. Each operation is synchronous."
            .to_string(),
        format!(
            "A subclass implements ``forward`` and declares itself the service's handler \
             with ``@nexusrpc.handler.service_handler(service={})``, from the services \
             module. nexusrpc reads that declaration from the class itself, never from a \
             base class.",
            service.name
        ),
    ];
    if service.deprecated {
        paragraphs.push("This service is deprecated.".to_string());
    }
    render_docstring(output, "    ", &paragraphs);
    output.push_str(
        r#"
    @abc.abstractmethod
    async def forward(
        self,
        operation: str,
        ctx: nexusrpc.handler.StartOperationContext,
        transfer: typing.Any,
    ) -> typing.Any:
        """Serves one operation from its input's JSON transfer form.

        ``operation`` is the operation's wire name, and ``transfer`` is its input
        as JSON values, or ``None`` when the operation takes none. The answer is
        the output's JSON transfer form, or ``None`` when the operation has none.
        Raise ``nexusrpc.HandlerError`` to fail the operation.
        """
        raise NotImplementedError
"#,
    );
    for operation in &service.operations {
        output.push('\n');
        render_operation(output, operation);
    }
}

fn render_operation(output: &mut String, operation: &ClientOperation) {
    output.push_str("    @nexusrpc.handler.sync_operation\n");
    output.push_str("    async def ");
    output.push_str(&operation.name);
    output.push_str("(\n        self,\n");
    output.push_str("        ctx: nexusrpc.handler.StartOperationContext,\n");
    output.push_str("        input: ");
    output.push_str(
        operation
            .input
            .as_ref()
            .map_or("None", |model| model.type_name.as_str()),
    );
    output.push_str(",\n    ) -> ");
    output.push_str(
        operation
            .output
            .as_ref()
            .map_or("None", |model| model.type_name.as_str()),
    );
    output.push_str(":\n");
    let mut lines = Vec::new();
    if let Some(doc) = &operation.doc {
        lines.push(doc.clone());
    }
    if operation.deprecated {
        lines.push("This operation is deprecated.".to_string());
    }
    if !lines.is_empty() {
        render_docstring(output, "        ", &lines);
    }
    match &operation.input {
        Some(model) => {
            output.push_str("        transfer: typing.Any = ");
            output.push_str(&converter_class_name(&model.model_name));
            output.push_str("().to_transfer_type(input)\n");
        }
        None => output.push_str("        transfer: typing.Any = None\n"),
    }
    let call = format!(
        "await self.forward({}, ctx, transfer)",
        literal(&operation.wire_name)
    );
    match &operation.output {
        Some(model) => {
            output.push_str("        answer: typing.Any = ");
            output.push_str(&call);
            output.push('\n');
            output.push_str("        return ");
            output.push_str(&converter_class_name(&model.model_name));
            output.push_str("().from_transfer_type(answer, ");
            output.push_str(&model.type_name);
            output.push_str(")\n");
        }
        None => {
            output.push_str("        ");
            output.push_str(&call);
            output.push('\n');
        }
    }
}
