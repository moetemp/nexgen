//! The generated Python HTTP caller (`--client`).
//!
//! This is a different caller from the `{Service}Client` the NativeApi surface
//! emits: that one starts a Nexus operation from inside a workflow, while this
//! one is for a process outside the worker posting to the Nexus HTTP ingress.
//! It carries no dependency beyond the standard library and the models module
//! next to it, because a stream producer is often not a Temporal worker at all.

use serde_json::Value;

use crate::generator::json_schema::client::{
    ClientHandle, ClientHandleKey, ClientLongPoll, ClientModel, ClientOperation, ClientPlan,
    ClientService,
};
use crate::generator::json_schema::python::converter_class_name;
use crate::generator::python::{python_field_name, render_generated_file_header};

/// The file the caller is emitted to, next to the models it uses.
pub(in crate::generator) const CLIENT_MODULE: &str = "client.py";

fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("a Rust string is always JSON-serializable")
}

/// The emitted attribute for a wire member: the `x-py-name` override if the
/// member carries one, otherwise the snake-cased wire name. This mirrors the
/// models module's own rule, so the caller reads the field the models declare.
fn member_attribute(member: Option<&Value>, wire_name: &str) -> String {
    member
        .and_then(|member| member.get("x-py-name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| python_field_name(wire_name))
}

fn client_class_name(service: &ClientService) -> String {
    format!("{}HttpClient", service.name)
}

/// The looping method's name. Deriving it from the result member keeps the
/// contract visible at the call site: an operation whose emptiness test is
/// `records` reads as `read_until_records`.
fn long_poll_method_name(
    operation: &ClientOperation,
    long_poll: &ClientLongPoll,
    output: &ClientModel,
) -> String {
    format!(
        "{}_until_{}",
        operation.name,
        member_attribute(
            output.member(&long_poll.result_member),
            &long_poll.result_member
        )
    )
}

/// Renders the whole module, or `None` when the module declares no service.
pub(in crate::generator) fn render_client_module(plan: &ClientPlan) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    let mut output = String::new();
    render_generated_file_header(&mut output);
    output.push('\n');
    render_imports(&mut output, plan);
    render_model_imports(&mut output, plan);
    render_post_helper(&mut output);
    if plan.has_long_poll() {
        render_long_poll_runtime(&mut output);
    }
    for service in &plan.services {
        render_service_client(&mut output, service);
        for handle in &service.handles {
            render_handle_class(&mut output, service, handle);
        }
    }
    Some(output)
}

fn render_imports(output: &mut String, plan: &ClientPlan) {
    output.push_str("import asyncio\n");
    if plan.has_long_poll() {
        output.push_str("import dataclasses\n");
    }
    if handle_annotation_names(plan)
        .iter()
        .any(|name| name.starts_with("datetime."))
    {
        output.push_str("import datetime\n");
    }
    output.push_str("import json\n");
    output.push_str("import ssl\n");
    if plan.has_long_poll() {
        output.push_str("import time\n");
    }
    output.push_str("import typing\n");
    output.push_str("import urllib.error\n");
    output.push_str("import urllib.request\n");
    // The models module already imports the SDK, so the codec hook adds no dependency.
    output.push_str("import temporalio.api.common.v1\n");
    output.push_str("import temporalio.converter\n\n");
}

/// The names the handles' parameter annotations use, which the flat caller's
/// own signatures do not: a handle spells out its keys and free members by the
/// models' field types. String literals (`typing.Literal` values) are not
/// names and are skipped.
fn handle_annotation_names(plan: &ClientPlan) -> std::collections::BTreeSet<String> {
    let mut annotations = Vec::new();
    for service in &plan.services {
        for handle in &service.handles {
            annotations.extend(handle.keys.iter().map(|key| key.type_name.clone()));
            for index in &handle.operations {
                if let Some(input) = &service.operations[*index].input {
                    annotations.extend(
                        input
                            .members()
                            .into_iter()
                            .filter_map(|(wire, _)| input.member_type(wire).ok()),
                    );
                }
            }
        }
    }
    let mut names = std::collections::BTreeSet::new();
    for annotation in annotations {
        let mut unquoted = String::new();
        let mut quote = None;
        let mut escaped = false;
        for ch in annotation.chars() {
            match quote {
                Some(open) => {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == open {
                        quote = None;
                    }
                }
                None if ch == '"' || ch == '\'' => {
                    quote = Some(ch);
                    unquoted.push(' ');
                }
                None => unquoted.push(ch),
            }
        }
        names.extend(
            unquoted
                .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.'))
                .filter(|token| token.starts_with(|c: char| c.is_alphabetic() || c == '_'))
                .map(str::to_string),
        );
    }
    names
}

/// A bare annotation name the module does not have to import.
fn is_python_builtin_name(name: &str) -> bool {
    matches!(
        name,
        "None"
            | "True"
            | "False"
            | "str"
            | "int"
            | "float"
            | "bool"
            | "bytes"
            | "list"
            | "dict"
            | "tuple"
            | "set"
            | "frozenset"
            | "object"
    )
}

fn render_model_imports(output: &mut String, plan: &ClientPlan) {
    let mut names = std::collections::BTreeSet::new();
    names.extend(
        handle_annotation_names(plan)
            .into_iter()
            .filter(|name| !name.contains('.') && !is_python_builtin_name(name)),
    );
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

fn render_long_poll_runtime(output: &mut String) {
    output.push_str(
        r#"_LONG_POLL_MIN_BACKOFF = 0.05
_LONG_POLL_MAX_BACKOFF = 2.0


async def _long_poll_pause(backoff: float, end: float) -> None:
    """Waits out a backoff without running past the read's own deadline."""
    remaining = end - time.monotonic()
    delay = min(backoff, remaining)
    if delay > 0:
        await asyncio.sleep(delay)


def _long_poll_next_backoff(backoff: float) -> float:
    return min(backoff * 2, _LONG_POLL_MAX_BACKOFF)


"#,
    );
}

fn render_poll_budget_method(output: &mut String) {
    output.push_str(
        r#"
    def _poll_budget(self, remaining: float) -> float:
        """How long one attempt may ask the endpoint to park, in seconds.

        Asking for longer than the transport will wait is a socket timeout
        dressed as a long poll, so the ask leaves the answer room to come back
        inside this client's own timeout.
        """
        return max(0.001, min(remaining, self._timeout * 0.9))
"#,
    );
}

fn render_post_helper(output: &mut String) {
    output.push_str(
        r#"class HTTPStatusError(RuntimeError):
    """An answer from the ingress that is not a synchronous result.

    It carries the status so a caller can tell a refusal apart from an answer
    another attempt could still get.
    """

    def __init__(
        self, url: str, status: int, detail: str, retryable: bool | None = None
    ) -> None:
        super().__init__(f"{url} failed ({status}): {detail}")
        # Annotated explicitly: a target's strict type checker asks for it on a
        # class it cannot prove closed.
        self.url: str = url
        self.status: int = status
        self.detail: str = detail
        self._retryable: bool | None = retryable

    @property
    def retryable(self) -> bool:
        """Whether the same request could answer differently.

        The handler's ``Nexus-Request-Retryable`` header decides when it is
        set. Otherwise too many requests and the server-side failures are the
        endpoint's own transient conditions; every other status is about this
        request, and repeating it changes nothing.
        """
        if self._retryable is not None:
            return self._retryable
        return self.status == 429 or self.status >= 500


def _retryable_header(headers: typing.Any) -> bool | None:
    value = headers.get("Nexus-Request-Retryable") if headers is not None else None
    if value is None:
        return None
    return str(value).strip().lower() == "true"


_PAYLOAD_CONTENT_TYPE = "application/x-temporal-payload"


def _post(
    url: str,
    body: bytes,
    headers: dict[str, str],
    timeout: float,
    content_type: str = "application/json",
    ssl_context: ssl.SSLContext | None = None,
) -> tuple[bytes, str]:
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": content_type,
            # Tells the handler how long this caller waits, so it can cut a
            # long poll short instead of being cut off.
            "Request-Timeout": f"{int(timeout * 1000)}ms",
            **headers,
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(
            request, timeout=timeout, context=ssl_context
        ) as response:
            status = typing.cast("int", response.status)
            answer = typing.cast("bytes", response.read())
            retryable = _retryable_header(response.headers)
            answer_type = str(response.headers.get("Content-Type") or "")
    except urllib.error.HTTPError as error:
        detail = error.read().decode(errors="replace")
        raise HTTPStatusError(
            url, error.code, detail, _retryable_header(error.headers)
        ) from error
    # A 201 means the handler started the operation asynchronously and answered
    # with a token, which this caller cannot read as the operation's result.
    if status not in (200, 204):
        raise HTTPStatusError(url, status, answer.decode(errors="replace"), retryable)
    return answer, answer_type


def _single_payload(
    url: str, payloads: list[temporalio.api.common.v1.Payload]
) -> temporalio.api.common.v1.Payload:
    if len(payloads) != 1:
        raise ValueError(
            f"{url}: the payload codec gave {len(payloads)} payloads for one"
        )
    return payloads[0]


async def _exchange(
    url: str,
    body: bytes,
    headers: dict[str, str],
    timeout: float,
    ssl_context: ssl.SSLContext | None,
    payload_codec: temporalio.converter.PayloadCodec | None,
) -> bytes:
    """Posts a JSON body and answers with the JSON the endpoint sent back.

    With a codec, the body crosses as an encoded Temporal payload, which the
    handler's own codec decodes. The ingress sends a result that the handler's
    codec encoded as a payload, and this caller decodes it before it parses.
    """
    content_type = "application/json"
    if payload_codec is not None:
        plain = temporalio.api.common.v1.Payload(
            metadata={"encoding": b"json/plain"}, data=body
        )
        encoded = _single_payload(url, await payload_codec.encode([plain]))
        body = encoded.SerializeToString()
        content_type = _PAYLOAD_CONTENT_TYPE
    answer, answer_type = await asyncio.to_thread(
        _post,
        url,
        body,
        headers,
        timeout,
        content_type=content_type,
        ssl_context=ssl_context,
    )
    if answer_type.split(";")[0].strip().lower() != _PAYLOAD_CONTENT_TYPE:
        return answer
    payload = temporalio.api.common.v1.Payload.FromString(answer)
    if payload_codec is not None:
        payload = _single_payload(url, await payload_codec.decode([payload]))
    encoding = payload.metadata.get("encoding", b"").decode(errors="replace")
    # A void result crosses as a null payload, which has no JSON to parse.
    if encoding == "binary/null":
        return b""
    if not encoding.startswith("json/"):
        hint = ", so pass payload_codec= to decode it" if payload_codec is None else ""
        raise ValueError(f"{url} answered with a {encoding!r} payload{hint}")
    return payload.data


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
        "Every method posts to ``{base_url}/{operation}``, so ``base_url`` is the service's \
         address on the Nexus HTTP ingress, up to but not including the operation name."
            .to_string(),
        "``ssl_context`` sets the TLS for an ``https`` ``base_url``, for example a private \
         certificate authority or a client certificate for mTLS."
            .to_string(),
        "Give ``payload_codec`` the codec of the namespace that runs the handler. The input \
         then crosses as an encoded payload, and an encoded result is decoded before it is \
         parsed."
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
    output.push_str("        ssl_context: ssl.SSLContext | None = None,\n");
    output.push_str("        payload_codec: temporalio.converter.PayloadCodec | None = None,\n");
    output.push_str("    ) -> None:\n");
    // Annotated explicitly: a target's strict type checker asks for it on a class
    // it cannot prove closed.
    output.push_str("        self._base_url: str = base_url.rstrip(\"/\")\n");
    output.push_str("        self._headers: dict[str, str] = dict(headers or {})\n");
    output.push_str("        self._timeout: float = timeout\n");
    output.push_str("        self._ssl_context: ssl.SSLContext | None = ssl_context\n");
    output.push_str(
        "        self._payload_codec: temporalio.converter.PayloadCodec | None = payload_codec\n",
    );

    if service
        .operations
        .iter()
        .any(|operation| operation.long_poll.is_some())
    {
        render_poll_budget_method(output);
    }

    for operation in &service.operations {
        output.push('\n');
        render_operation_method(output, operation);
        if let Some(long_poll) = &operation.long_poll
            && let Some(input) = &operation.input
            && let Some(model_output) = &operation.output
        {
            output.push('\n');
            render_long_poll_method(output, operation, long_poll, input, model_output);
        }
    }
    for handle in &service.handles {
        output.push('\n');
        render_handle_constructor(output, handle, None, &handle.constructor_base, "self");
    }
    output.push('\n');
}

// ---------------------------------------------------------------------------
// Handle projection (`x-nexus-handle`)
// ---------------------------------------------------------------------------

fn key_attribute(key: &ClientHandleKey) -> String {
    member_attribute(Some(&key.property), &key.wire_name)
}

fn key_takes_default(key: &ClientHandleKey) -> bool {
    !key.required || key.property.get("default").is_some()
}

fn constructor_name(base: &str) -> String {
    python_field_name(base)
}

/// The constructor of `target` emitted on the flat caller (`parent` is `None`)
/// or on a parent handle, which already holds every key but `extra`.
///
/// `client_expr` is how the body reaches the flat caller from where the method
/// sits: `self` on the caller itself, `self._client` on a handle.
fn render_handle_constructor(
    output: &mut String,
    target: &ClientHandle,
    parent: Option<(&ClientHandle, &[usize])>,
    base: &str,
    client_expr: &str,
) {
    let taken: Vec<&ClientHandleKey> = match parent {
        Some((_, extra)) => extra.iter().map(|index| &target.keys[*index]).collect(),
        None => target.keys.iter().collect(),
    };
    output.push_str("    def ");
    output.push_str(&constructor_name(base));
    output.push_str("(\n        self,\n");
    if !taken.is_empty() {
        output.push_str("        *,\n");
    }
    for key in &taken {
        output.push_str("        ");
        output.push_str(&key_attribute(key));
        output.push_str(": ");
        output.push_str(&key.type_name);
        if key_takes_default(key) {
            output.push_str(" = None");
        }
        output.push_str(",\n");
    }
    output.push_str("    ) -> \"");
    output.push_str(&target.name);
    output.push_str("\":\n");
    let bound = target
        .keys
        .iter()
        .map(|key| format!("``{}``", key_attribute(key)))
        .collect::<Vec<_>>()
        .join(", ");
    render_docstring(
        output,
        "        ",
        &[match parent {
            Some(_) => format!(
                "A ``{}`` bound to {bound}: this handle's own members and the ones given here.",
                target.name
            ),
            None => format!("A ``{}`` bound to {bound}.", target.name),
        }],
    );
    output.push_str("        return ");
    output.push_str(&target.name);
    output.push_str("(\n            ");
    output.push_str(client_expr);
    output.push_str(",\n");
    for key in &target.keys {
        let attribute = key_attribute(key);
        output.push_str("            ");
        output.push_str(&attribute);
        output.push('=');
        if taken.iter().any(|given| given.wire_name == key.wire_name) {
            output.push_str(&attribute);
        } else {
            output.push_str("self._");
            output.push_str(&attribute);
        }
        output.push_str(",\n");
    }
    output.push_str("        )\n");
}

fn render_handle_class(output: &mut String, service: &ClientService, handle: &ClientHandle) {
    let client_class = client_class_name(service);
    output.push_str("\nclass ");
    output.push_str(&handle.name);
    output.push_str(":\n");
    let bound = handle
        .keys
        .iter()
        .map(|key| format!("``{}``", key_attribute(key)))
        .collect::<Vec<_>>()
        .join(", ");
    render_docstring(
        output,
        "    ",
        &[
            format!("Binds {bound} for the {} service.", service.wire_name),
            format!(
                "Every method posts through the ``{client_class}`` it was built from with the \
                 bound members filled in, so a caller states a stream's identity once. The \
                 flat caller stays available for a call that spells every member out."
            ),
        ],
    );
    output.push_str("\n    def __init__(\n        self,\n        client: ");
    output.push_str(&client_class);
    output.push_str(",\n");
    if !handle.keys.is_empty() {
        output.push_str("        *,\n");
    }
    for key in &handle.keys {
        output.push_str("        ");
        output.push_str(&key_attribute(key));
        output.push_str(": ");
        output.push_str(&key.type_name);
        if key_takes_default(key) {
            output.push_str(" = None");
        }
        output.push_str(",\n");
    }
    output.push_str("    ) -> None:\n");
    // Annotated explicitly: a target's strict type checker asks for it on a
    // class it cannot prove closed.
    output.push_str("        self._client: ");
    output.push_str(&client_class);
    output.push_str(" = client\n");
    for key in &handle.keys {
        let attribute = key_attribute(key);
        output.push_str("        self._");
        output.push_str(&attribute);
        output.push_str(": ");
        output.push_str(&key.type_name);
        output.push_str(" = ");
        output.push_str(&attribute);
        output.push('\n');
    }

    for index in &handle.operations {
        let operation = &service.operations[*index];
        let Some(input) = &operation.input else {
            continue;
        };
        output.push('\n');
        render_handle_method(output, handle, operation, input, None);
        if let Some(long_poll) = &operation.long_poll
            && let Some(model_output) = &operation.output
        {
            output.push('\n');
            render_handle_method(
                output,
                handle,
                operation,
                input,
                Some((long_poll, model_output)),
            );
        }
    }
    for child in &handle.children {
        output.push('\n');
        render_handle_constructor(
            output,
            &service.handles[child.handle],
            Some((handle, &child.extra_keys)),
            &child.constructor_base,
            "self._client",
        );
    }
    output.push('\n');
}

/// One operation as a handle method: the key members come off the handle, the
/// rest stay as keyword parameters, and the call goes through the flat caller
/// so codec and transport live in one place. With `long_poll` it is the loop
/// method, whose wait member the loop sets, so it takes no parameter for it.
fn render_handle_method(
    output: &mut String,
    handle: &ClientHandle,
    operation: &ClientOperation,
    input: &ClientModel,
    long_poll: Option<(&ClientLongPoll, &ClientModel)>,
) {
    let method = match long_poll {
        Some((long_poll, model_output)) => {
            long_poll_method_name(operation, long_poll, model_output)
        }
        None => operation.name.clone(),
    };
    let wait_member = long_poll.map(|(long_poll, _)| long_poll.wait_member.as_str());
    let free: Vec<(&str, &Value)> = input
        .members()
        .into_iter()
        .filter(|(wire, _)| {
            !handle.binds(wire) && !input.member_is_const(wire) && Some(*wire) != wait_member
        })
        .collect();
    let return_annotation = operation
        .output
        .as_ref()
        .map(|model| model.type_name.clone())
        .unwrap_or_else(|| "None".to_string());

    output.push_str("    async def ");
    output.push_str(&method);
    output.push_str("(\n        self,\n");
    if !free.is_empty() || long_poll.is_some() {
        output.push_str("        *,\n");
    }
    for (wire, member) in &free {
        output.push_str("        ");
        output.push_str(&member_attribute(Some(member), wire));
        output.push_str(": ");
        output.push_str(&input.member_type(wire).unwrap_or_default());
        if input.member_takes_default(wire) {
            output.push_str(" = None");
        }
        output.push_str(",\n");
    }
    if long_poll.is_some() {
        output.push_str("        deadline: float,\n");
    }
    output.push_str("    ) -> ");
    output.push_str(&return_annotation);
    output.push_str(":\n");
    let mut lines = Vec::new();
    match long_poll {
        Some((long_poll, model_output)) => {
            lines.extend(long_poll_paragraphs(
                operation,
                long_poll,
                input,
                model_output,
            ));
        }
        None => lines.extend(operation.doc.clone()),
    }
    let bound = handle
        .keys
        .iter()
        .map(|key| format!("``{}``", key_attribute(key)))
        .collect::<Vec<_>>()
        .join(", ");
    let pronoun = if handle.keys.len() == 1 { "it" } else { "them" };
    lines.push(format!(
        "This handle supplies {bound}; the flat caller's ``{method}`` takes {pronoun} spelled out."
    ));
    if operation.deprecated {
        lines.push("This operation is deprecated.".to_string());
    }
    render_docstring(output, "        ", &lines);
    output.push_str("        return await self._client.");
    output.push_str(&method);
    output.push_str("(\n            ");
    output.push_str(&input.type_name);
    output.push_str("(\n");
    for key in &handle.keys {
        let attribute = key_attribute(key);
        output.push_str("                ");
        output.push_str(&attribute);
        output.push_str("=self._");
        output.push_str(&attribute);
        output.push_str(",\n");
    }
    for (wire, member) in &free {
        let attribute = member_attribute(Some(member), wire);
        output.push_str("                ");
        output.push_str(&attribute);
        output.push('=');
        output.push_str(&attribute);
        output.push_str(",\n");
    }
    output.push_str("            )");
    if long_poll.is_some() {
        output.push_str(",\n            deadline=deadline");
    }
    output.push_str(",\n        )\n");
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
        "        raw = await _exchange(\n"
    } else {
        "        await _exchange(\n"
    });
    output.push_str("            f\"{self._base_url}/");
    output.push_str(&operation.path_segment);
    output.push_str("\",\n");
    output.push_str("            body,\n");
    output.push_str("            self._headers,\n");
    output.push_str("            self._timeout,\n");
    output.push_str("            self._ssl_context,\n");
    output.push_str("            self._payload_codec,\n");
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

fn render_long_poll_method(
    output: &mut String,
    operation: &ClientOperation,
    long_poll: &ClientLongPoll,
    input: &ClientModel,
    model_output: &ClientModel,
) {
    output.push_str("    async def ");
    output.push_str(&long_poll_method_name(operation, long_poll, model_output));
    output.push_str("(\n        self,\n        request: ");
    output.push_str(&input.type_name);
    output.push_str(",\n        *,\n        deadline: float,\n    ) -> ");
    output.push_str(&model_output.type_name);
    output.push_str(":\n");
    render_docstring(
        output,
        "        ",
        &long_poll_paragraphs(operation, long_poll, input, model_output),
    );
    render_long_poll_body(output, operation, long_poll, input, model_output);
}

/// What a loop method does, shared by the flat caller and every handle that
/// carries the operation.
fn long_poll_paragraphs(
    operation: &ClientOperation,
    long_poll: &ClientLongPoll,
    input: &ClientModel,
    model_output: &ClientModel,
) -> Vec<String> {
    let wait_attribute =
        member_attribute(input.member(&long_poll.wait_member), &long_poll.wait_member);
    let result_attribute = member_attribute(
        model_output.member(&long_poll.result_member),
        &long_poll.result_member,
    );
    let done_attribute = long_poll
        .done_member
        .as_ref()
        .map(|(done, _)| member_attribute(model_output.member(done), done));
    let mut paragraphs = vec![
        format!(
            "Calls ``{}`` until it answers with something, or until ``deadline``.",
            operation.name
        ),
        format!(
            "Each attempt sets ``{wait_attribute}`` from the budget left, so the endpoint \
                 parks for the caller instead of the caller spinning. The first answer whose \
                 ``{result_attribute}`` is non-empty{done} is returned. When the deadline passes \
                 first, the last answer is returned, which still carries the caller's resume \
                 position.",
            done = done_attribute
                .as_ref()
                .map(|done| format!(", or whose ``{done}`` is true,"))
                .unwrap_or_default(),
        ),
        "``deadline`` is the whole budget in seconds. The single-shot method stays \
             available for a caller that wants one attempt."
            .to_string(),
    ];
    if let Some(state) = &long_poll.state_member {
        paragraphs.insert(
            2,
            format!(
                "After an answer with nothing, the next attempt sends the ``{}`` that answer \
                 carried, so the endpoint keeps nothing between calls.",
                member_attribute(input.member(state), state)
            ),
        );
    }
    paragraphs
}

fn render_long_poll_body(
    output: &mut String,
    operation: &ClientOperation,
    long_poll: &ClientLongPoll,
    input: &ClientModel,
    model_output: &ClientModel,
) {
    let wait_attribute =
        member_attribute(input.member(&long_poll.wait_member), &long_poll.wait_member);
    let result_attribute = member_attribute(
        model_output.member(&long_poll.result_member),
        &long_poll.result_member,
    );
    let done_attribute = long_poll
        .done_member
        .as_ref()
        .map(|(done, _)| member_attribute(model_output.member(done), done));
    output.push_str("        end = time.monotonic() + deadline\n");
    output.push_str("        answer: ");
    output.push_str(&model_output.type_name);
    output.push_str(" | None = None\n");
    output.push_str("        backoff = _LONG_POLL_MIN_BACKOFF\n");
    output.push_str("        while True:\n");
    output.push_str("            wait = self._poll_budget(end - time.monotonic())\n");
    output.push_str("            wait_ms = int(wait * 1000)\n");
    // The model refuses a wait outside the member's declared range.
    if let Some(maximum) = long_poll.wait_maximum {
        output.push_str(&format!("            wait_ms = min(wait_ms, {maximum})\n"));
    }
    if let Some(minimum) = long_poll.wait_minimum {
        output.push_str(&format!("            wait_ms = max(wait_ms, {minimum})\n"));
    }
    if long_poll.wait_maximum.is_some() || long_poll.wait_minimum.is_some() {
        output.push_str("            wait = wait_ms / 1000\n");
    }
    output.push_str("            attempt = dataclasses.replace(\n");
    output.push_str("                request, ");
    output.push_str(&wait_attribute);
    output.push_str("=wait_ms\n");
    output.push_str("            )\n");
    output.push_str("            started = time.monotonic()\n");
    output.push_str("            try:\n");
    output.push_str("                answer = await self.");
    output.push_str(&operation.name);
    output.push_str("(attempt)\n");
    output.push_str("            except HTTPStatusError as error:\n");
    output.push_str(
        "                if not error.retryable or time.monotonic() >= end:\n                    raise\n",
    );
    output.push_str("                await _long_poll_pause(backoff, end)\n");
    output.push_str("                backoff = _long_poll_next_backoff(backoff)\n");
    output.push_str("                continue\n");
    output.push_str("            if answer.");
    output.push_str(&result_attribute);
    if let Some(done) = &done_attribute {
        output.push_str(" or answer.");
        output.push_str(done);
    }
    output.push_str(":\n");
    output.push_str("                return answer\n");
    if let Some(state) = &long_poll.state_member {
        let input_state = member_attribute(input.member(state), state);
        let output_state = member_attribute(model_output.member(state), state);
        // An answer with no state leaves the caller's own.
        output.push_str("            if answer.");
        output.push_str(&output_state);
        output.push_str(" is not None:\n");
        output.push_str("                request = dataclasses.replace(request, ");
        output.push_str(&input_state);
        output.push_str("=answer.");
        output.push_str(&output_state);
        output.push_str(")\n");
    }
    output.push_str("            if time.monotonic() >= end:\n");
    output.push_str("                return answer\n");
    // An endpoint that answers empty without parking for the wait it was given
    // would otherwise be asked again as fast as the network allows.
    output.push_str("            if time.monotonic() - started < wait / 2:\n");
    output.push_str("                await _long_poll_pause(backoff, end)\n");
    output.push_str("                backoff = _long_poll_next_backoff(backoff)\n");
    output.push_str("            else:\n");
    output.push_str("                backoff = _LONG_POLL_MIN_BACKOFF\n");
}

/// A docstring with one paragraph per entry. The shared Python docstring writer
/// takes a single summary plus tagged sections, which is the wrong shape for
/// prose paragraphs.
pub(in crate::generator) fn render_docstring(
    output: &mut String,
    indent: &str,
    paragraphs: &[String],
) {
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
