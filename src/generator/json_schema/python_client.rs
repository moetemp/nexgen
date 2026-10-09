//! The generated Python HTTP caller (`--client`).
//!
//! This is a different caller from the `{Service}Client` the NativeApi surface
//! emits: that one starts a Nexus operation from inside a workflow, while this
//! one is for a process outside the worker posting to the Nexus HTTP ingress.
//! It carries no dependency beyond the standard library and the models module
//! next to it, because a stream producer is often not a Temporal worker at all.

use crate::generator::json_schema::client::{ClientOperation, ClientPlan, ClientService};
use crate::generator::json_schema::python::converter_class_name;
use crate::generator::python::render_generated_file_header;

/// The file the caller is emitted to, next to the models it uses.
pub(in crate::generator) const CLIENT_MODULE: &str = "client.py";

fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("a Rust string is always JSON-serializable")
}

fn client_class_name(service: &ClientService) -> String {
    format!("{}HttpClient", service.name)
}

/// Renders the whole module, or `None` when the module declares no service.
pub(in crate::generator) fn render_client_module(plan: &ClientPlan) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    let mut output = String::new();
    render_generated_file_header(&mut output);
    output.push('\n');
    render_imports(&mut output);
    render_model_imports(&mut output, plan);
    render_post_helper(&mut output);
    for service in &plan.services {
        render_service_client(&mut output, service);
    }
    Some(output)
}

fn render_imports(output: &mut String) {
    output.push_str("import asyncio\n");
    output.push_str("import json\n");
    output.push_str("import typing\n");
    output.push_str("import urllib.error\n");
    output.push_str("import urllib.request\n\n");
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
    // The converters are the models module's own private classes. A generated
    // caller is versioned with the models it sits beside, so reaching for them
    // is a package-internal call rather than a dependency on private API.
    output.push_str("from .models import (\n");
    for name in &names {
        output.push_str("    ");
        output.push_str(name);
        output.push_str(",\n");
    }
    output.push_str(")\n\n\n");
}

fn render_post_helper(output: &mut String) {
    output.push_str(
        r#"class HTTPStatusError(RuntimeError):
    """An answer from the ingress that is not a synchronous result.

    It carries the status so a caller can tell a refusal apart from an answer
    another attempt could still get.
    """

    def __init__(self, url: str, status: int, detail: str) -> None:
        super().__init__(f"{url} failed ({status}): {detail}")
        # Annotated explicitly: a target's strict type checker asks for it on a
        # class it cannot prove closed.
        self.url: str = url
        self.status: int = status
        self.detail: str = detail

    @property
    def retryable(self) -> bool:
        """Whether the same request could answer differently.

        Too many requests and the server-side failures are the endpoint's own
        transient conditions; every other status is about this request, and
        repeating it changes nothing.
        """
        return self.status == 429 or self.status >= 500


def _post(
    url: str, body: bytes, headers: dict[str, str], timeout: float
) -> bytes:
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": "application/json", **headers},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            status = typing.cast("int", response.status)
            answer = typing.cast("bytes", response.read())
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise HTTPStatusError(url, error.code, detail) from error
    # A 201 means the handler started the operation asynchronously and answered
    # with a token, which this caller cannot read as the operation's result.
    if status not in (200, 204):
        raise HTTPStatusError(url, status, answer.decode(errors="replace"))
    return answer


"#,
    );
}

fn render_service_client(output: &mut String, service: &ClientService) {
    let class_name = client_class_name(service);
    output.push_str("class ");
    output.push_str(&class_name);
    output.push_str(":\n");
    let mut paragraphs = vec![
        service
            .doc
            .clone()
            .unwrap_or_else(|| format!("An HTTP caller for the {} service.", service.wire_name)),
        "Every method posts to `{base_url}/{operation}`, so `base_url` is the service's \
         address on the Nexus HTTP ingress, up to but not including the operation name."
            .to_string(),
    ];
    if service.deprecated {
        paragraphs.push("This service is deprecated.".to_string());
    }
    render_docstring(output, "    ", &paragraphs);
    output.push_str("\n    def __init__(\n");
    output.push_str("        self,\n");
    output.push_str("        base_url: str,\n");
    output.push_str("        *,\n");
    output.push_str("        headers: dict[str, str] | None = None,\n");
    output.push_str("        timeout: float = 30.0,\n");
    output.push_str("    ) -> None:\n");
    // Annotated explicitly: a target's strict type checker asks for it on a class
    // it cannot prove closed.
    output.push_str("        self._base_url: str = base_url.rstrip(\"/\")\n");
    output.push_str("        self._headers: dict[str, str] = dict(headers or {})\n");
    output.push_str("        self._timeout: float = timeout\n");

    for operation in &service.operations {
        output.push('\n');
        render_operation_method(output, operation);
    }
    output.push('\n');
}

fn render_operation_method(output: &mut String, operation: &ClientOperation) {
    let return_annotation = operation
        .output
        .as_ref()
        .map(|model| model.type_name.clone())
        .unwrap_or_else(|| "None".to_string());
    output.push_str("    async def ");
    output.push_str(&operation.name);
    output.push_str("(\n        self,\n");
    if let Some(input) = &operation.input {
        output.push_str("        request: ");
        output.push_str(&input.type_name);
        output.push_str(",\n");
    }
    output.push_str("    ) -> ");
    output.push_str(&return_annotation);
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
        Some(input) => {
            output.push_str("        wire: typing.Any = ");
            output.push_str(&converter_class_name(&input.model_name));
            output.push_str("().to_transfer_type(request)\n");
            output.push_str("        body = json.dumps(wire).encode()\n");
        }
        None => output.push_str("        body = b\"{}\"\n"),
    }
    output.push_str(if operation.output.is_some() {
        "        raw = await asyncio.to_thread(\n"
    } else {
        "        await asyncio.to_thread(\n"
    });
    output.push_str("            _post,\n");
    output.push_str("            f\"{self._base_url}/");
    output.push_str(&operation.path_segment);
    output.push_str("\",\n");
    output.push_str("            body,\n");
    output.push_str("            self._headers,\n");
    output.push_str("            self._timeout,\n");
    output.push_str("        )\n");

    let Some(model_output) = &operation.output else {
        return;
    };
    // An operation that declares an output and answers with nothing has not
    // answered. Reading it as the all-defaults value would hand the caller a
    // value the endpoint never produced.
    output.push_str("        if not raw:\n");
    output.push_str("            raise ValueError(\n                ");
    output.push_str(&literal(&format!(
        "{} answered with an empty body",
        operation.wire_name
    )));
    output.push_str("\n            )\n");
    output.push_str("        answer: typing.Any = json.loads(raw)\n");
    output.push_str("        return ");
    output.push_str(&converter_class_name(&model_output.model_name));
    output.push_str("().from_transfer_type(answer, ");
    output.push_str(&model_output.type_name);
    output.push_str(")\n");
}

/// A docstring with one paragraph per entry. The shared Python docstring writer
/// takes a single summary plus tagged sections, which is the wrong shape for
/// prose paragraphs.
fn render_docstring(output: &mut String, indent: &str, paragraphs: &[String]) {
    let paragraphs = paragraphs
        .iter()
        .map(|paragraph| paragraph.trim())
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        return;
    }
    let lines = paragraphs
        .iter()
        .map(|paragraph| wrap(paragraph, 88 - indent.len() - 6))
        .collect::<Vec<_>>();
    output.push_str(indent);
    output.push_str("\"\"\"");
    // A docstring that fits on one line closes on it, which is what every
    // Python formatter would rewrite it to anyway.
    if let [only] = lines.as_slice()
        && let [line] = only.as_slice()
    {
        output.push_str(line);
        output.push_str("\"\"\"\n");
        return;
    }
    for (index, paragraph) in lines.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        for (line_index, line) in paragraph.iter().enumerate() {
            if index > 0 || line_index > 0 {
                output.push_str(indent);
            }
            output.push_str(line);
            output.push('\n');
        }
    }
    output.push_str(indent);
    output.push_str("\"\"\"\n");
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}
