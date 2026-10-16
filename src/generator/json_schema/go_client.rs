//! The generated Go HTTP caller (`--client`).
//!
//! This is a different caller from the `{Service}Client` the NativeApi surface
//! emits: that one starts a Nexus operation from inside a workflow, while this
//! one is for a process outside the worker posting to the Nexus HTTP ingress.
//! It depends on the standard library only, because a stream producer is often
//! not a Temporal worker at all.

use serde_json::Value;

use crate::generator::go::{
    GENERATED_HEADER, go_field_name, is_go_keyword,
    render_go_doc_comment as render_wrapped_go_doc_comment,
};
use crate::generator::json_schema::client::{
    ClientHandle, ClientHandleKey, ClientLongPoll, ClientModel, ClientOperation, ClientPlan,
    ClientService,
};

/// The file the caller is emitted to, in the same flat package as the models.
pub(in crate::generator) const CLIENT_FILE: &str = "client.go";

fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("a Rust string is always JSON-serializable")
}

/// The emitted field for a wire member: the `x-go-name` override if the member
/// carries one, otherwise the PascalCased wire name. This mirrors the models
/// file's own rule, so the caller reads the field the structs declare.
fn member_field(member: Option<&Value>, wire_name: &str) -> String {
    member
        .and_then(|member| member.get("x-go-name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| go_field_name(wire_name))
}

fn client_type_name(service: &ClientService) -> String {
    format!("{}HTTPClient", service.name)
}

/// The looping method's name. Deriving it from the result member keeps the
/// contract visible at the call site: an operation whose emptiness test is
/// `records` reads as `ReadUntilRecords`.
fn long_poll_method_name(
    operation: &ClientOperation,
    long_poll: &ClientLongPoll,
    output: &ClientModel,
) -> String {
    format!(
        "{}Until{}",
        operation.name,
        member_field(
            output.member(&long_poll.result_member),
            &long_poll.result_member
        )
    )
}

/// Renders the whole file, or `None` when the module declares no service.
pub(in crate::generator) fn render_client_file(
    package_name: &str,
    plan: &ClientPlan,
) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    let mut output = String::new();
    output.push_str(GENERATED_HEADER);
    output.push_str("\n\npackage ");
    output.push_str(package_name);
    output.push_str("\n\n");
    render_imports(&mut output, plan);
    render_status_error(&mut output);
    if plan.has_long_poll() {
        render_long_poll_runtime(&mut output);
    }
    for service in &plan.services {
        render_service_client(&mut output, service);
        for handle in &service.handles {
            render_handle(&mut output, service, handle);
        }
    }
    Some(output)
}

// ---------------------------------------------------------------------------
// Handle projection (`x-nexus-handle`)
// ---------------------------------------------------------------------------

/// The struct field a key is stored under on the handle: the model's field
/// name with its first letter lowered, so it is unexported and reads like the
/// member it mirrors.
fn key_field(key: &ClientHandleKey) -> String {
    local_name(&member_field(Some(&key.property), &key.wire_name))
}

/// A parameter or field name derived from an exported field name. A Go keyword
/// takes a suffix rather than a leading underscore, which gofmt frowns on, and
/// so do the names a constructor already binds: its receiver (`c` on the flat
/// caller, `h` on a parent handle) and the handle's `client` field.
fn local_name(field: &str) -> String {
    let mut chars = field.chars();
    let mut name = match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    if is_go_keyword(&name) || matches!(name.as_str(), "client" | "c" | "h") {
        name.push_str("Value");
    }
    name
}

fn constructor_name(base: &str) -> String {
    go_field_name(base)
}

/// `expr`, held as `from`, as a value of `to`. Go names an inline enum after
/// the model that declares it, so one key can be two types over the same
/// underlying type on two operations; a conversion between them is legal, and
/// the loader has already refused a key whose shape differs.
fn converted(expr: &str, from: &str, to: &str) -> String {
    if from == to {
        expr.to_string()
    } else if to.starts_with('*') {
        format!("({to})({expr})")
    } else {
        format!("{to}({expr})")
    }
}

/// The constructor of `target` emitted on the flat caller (`parent` is `None`)
/// or on a parent handle, which already holds every key but `extra`.
fn render_handle_constructor(
    output: &mut String,
    client_type: &str,
    target: &ClientHandle,
    parent: Option<(&ClientHandle, &[usize])>,
    base: &str,
) {
    let taken: Vec<&ClientHandleKey> = match parent {
        Some((_, extra)) => extra.iter().map(|index| &target.keys[*index]).collect(),
        None => target.keys.iter().collect(),
    };
    let method = constructor_name(base);
    let bound = target
        .keys
        .iter()
        .map(|key| member_field(Some(&key.property), &key.wire_name))
        .collect::<Vec<_>>()
        .join(", ");
    let (receiver, receiver_type, client_expr) = match parent {
        Some((parent, _)) => ("h", parent.name.as_str(), "h.client"),
        None => ("c", client_type, "c"),
    };
    render_wrapped_go_doc_comment(
        output,
        "",
        &match parent {
            Some(_) => format!(
                "{method} binds {bound} into a {}: this handle's own members and the ones given here.",
                target.name
            ),
            None => format!("{method} binds {bound} into a {}.", target.name),
        },
    );
    output.push_str("func (");
    output.push_str(receiver);
    output.push_str(" *");
    output.push_str(receiver_type);
    output.push_str(") ");
    output.push_str(&method);
    output.push('(');
    for (index, key) in taken.iter().enumerate() {
        if index > 0 {
            output.push_str(", ");
        }
        output.push_str(&key_field(key));
        output.push(' ');
        output.push_str(&key.type_name);
    }
    output.push_str(") *");
    output.push_str(&target.name);
    output.push_str(" {\n\treturn &");
    output.push_str(&target.name);
    output.push_str("{\n\t\tclient: ");
    output.push_str(client_expr);
    output.push_str(",\n");
    for key in &target.keys {
        let field = key_field(key);
        output.push_str("\t\t");
        output.push_str(&field);
        output.push_str(": ");
        match parent {
            Some((parent, _)) if !taken.iter().any(|given| given.wire_name == key.wire_name) => {
                let held = parent
                    .keys
                    .iter()
                    .find(|held| held.wire_name == key.wire_name)
                    .map_or(key.type_name.as_str(), |held| held.type_name.as_str());
                output.push_str(&converted(&format!("h.{field}"), held, &key.type_name));
            }
            _ => output.push_str(&field),
        }
        output.push_str(",\n");
    }
    output.push_str("\t}\n}\n\n");
}

fn render_handle(output: &mut String, service: &ClientService, handle: &ClientHandle) {
    let client_type = client_type_name(service);
    let bound = handle
        .keys
        .iter()
        .map(|key| member_field(Some(&key.property), &key.wire_name))
        .collect::<Vec<_>>()
        .join(", ");
    render_wrapped_go_doc_comment(
        output,
        "",
        &format!(
            "{} binds {bound} for the {} service. Every method posts through the {client_type} \
             it was built from with the bound members filled in, so a caller states a stream's \
             identity once. The flat caller stays available for a call that spells every \
             member out.",
            handle.name, service.wire_name
        ),
    );
    output.push_str("type ");
    output.push_str(&handle.name);
    output.push_str(" struct {\n\tclient *");
    output.push_str(&client_type);
    output.push('\n');
    for key in &handle.keys {
        output.push('\t');
        output.push_str(&key_field(key));
        output.push(' ');
        output.push_str(&key.type_name);
        output.push('\n');
    }
    output.push_str("}\n\n");

    render_handle_constructor(output, &client_type, handle, None, &handle.constructor_base);

    for index in &handle.operations {
        let operation = &service.operations[*index];
        let Some(input) = &operation.input else {
            continue;
        };
        render_handle_request_type(output, handle, operation, input);
        render_handle_method(output, handle, operation, input, None);
        if let Some(long_poll) = &operation.long_poll
            && let Some(model_output) = &operation.output
        {
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
        render_handle_constructor(
            output,
            &client_type,
            &service.handles[child.handle],
            Some((handle, &child.extra_keys)),
            &child.constructor_base,
        );
    }
}

/// The request type a handle method takes for one operation: the operation's
/// input without the members the handle binds. It is not a wire type; the
/// method copies it into the operation's own input with the bound members
/// filled in, so the model's converter runs once, on the whole request.
fn handle_request_type_name(handle: &ClientHandle, operation: &ClientOperation) -> String {
    format!("{}{}Request", handle.name, operation.name)
}

fn render_handle_request_type(
    output: &mut String,
    handle: &ClientHandle,
    operation: &ClientOperation,
    input: &ClientModel,
) {
    let type_name = handle_request_type_name(handle, operation);
    let free: Vec<(&str, &Value)> = input
        .members()
        .into_iter()
        .filter(|(wire, _)| !handle.binds(wire))
        .collect();
    render_wrapped_go_doc_comment(
        output,
        "",
        &format!(
            "{type_name} holds the {} members that {} does not bind. It is not a wire type: \
             the handle copies it into {} with the bound members filled in.",
            input.type_name, handle.name, input.type_name
        ),
    );
    output.push_str("type ");
    output.push_str(&type_name);
    output.push_str(" struct {\n");
    let looped = operation.long_poll.as_ref().zip(operation.output.as_ref());
    for (wire, member) in &free {
        let field = member_field(Some(member), wire);
        if let Some((long_poll, model_output)) = looped
            && long_poll.wait_member == *wire
        {
            render_wrapped_go_doc_comment(
                output,
                "\t",
                &format!(
                    "{field} is for {}. {} ignores it and sets the wait on each attempt.",
                    operation.name,
                    long_poll_method_name(operation, long_poll, model_output)
                ),
            );
        }
        output.push('\t');
        output.push_str(&field);
        output.push(' ');
        output.push_str(&input.member_type(wire).unwrap_or_default());
        output.push('\n');
    }
    output.push_str("}\n\n");
}

/// One operation as a handle method: it takes the per-handle request type,
/// builds the operation's input from the bound members and the free ones, and
/// posts through the flat caller so codec and transport live in one place.
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
    let request_type = handle_request_type_name(handle, operation);
    let bound = handle
        .keys
        .iter()
        .map(|key| member_field(input.member(&key.wire_name), &key.wire_name))
        .collect::<Vec<_>>();
    render_wrapped_go_doc_comment(
        output,
        "",
        &format!(
            "{method} posts through the caller with the bound {} filled in from this handle.",
            bound.join(", ")
        ),
    );
    output.push_str("func (h *");
    output.push_str(&handle.name);
    output.push_str(") ");
    output.push_str(&method);
    output.push_str("(ctx context.Context, request ");
    output.push_str(&request_type);
    if long_poll.is_some() {
        output.push_str(", deadline time.Duration");
    }
    match &operation.output {
        Some(model_output) => {
            output.push_str(") (");
            output.push_str(&model_output.type_name);
            output.push_str(", error) {\n\tfull := ");
        }
        None => output.push_str(") error {\n\tfull := "),
    }
    output.push_str(&input.type_name);
    output.push_str("{\n");
    for (key, field) in handle.keys.iter().zip(&bound) {
        let wanted = input
            .member_type(&key.wire_name)
            .unwrap_or_else(|_| key.type_name.clone());
        output.push_str("\t\t");
        output.push_str(field);
        output.push_str(": ");
        output.push_str(&converted(
            &format!("h.{}", key_field(key)),
            &key.type_name,
            &wanted,
        ));
        output.push_str(",\n");
    }
    for (wire, member) in input.members() {
        if handle.binds(wire) {
            continue;
        }
        let field = member_field(Some(member), wire);
        output.push_str("\t\t");
        output.push_str(&field);
        output.push_str(": request.");
        output.push_str(&field);
        output.push_str(",\n");
    }
    output.push_str("\t}\n\treturn h.client.");
    output.push_str(&method);
    output.push_str("(ctx, full");
    if long_poll.is_some() {
        output.push_str(", deadline");
    }
    output.push_str(")\n}\n\n");
}

/// Go refuses an unused import, so `errors` is imported only when some
/// operation decodes an output, and `encoding/json` when one encodes or decodes.
fn render_imports(output: &mut String, plan: &ClientPlan) {
    let operations = || {
        plan.services
            .iter()
            .flat_map(|service| service.operations.iter())
    };
    let decodes = operations().any(|operation| operation.output.is_some());
    let encodes = operations().any(|operation| operation.input.is_some());
    output.push_str("import (\n");
    output.push_str("\t\"bytes\"\n");
    output.push_str("\t\"context\"\n");
    if encodes || decodes {
        output.push_str("\t\"encoding/json\"\n");
    }
    if decodes {
        output.push_str("\t\"errors\"\n");
    }
    output.push_str("\t\"fmt\"\n");
    output.push_str("\t\"io\"\n");
    output.push_str("\t\"net/http\"\n");
    output.push_str("\t\"strings\"\n");
    // Every request reads the transport timeout for its Request-Timeout header.
    output.push_str("\t\"time\"\n");
    output.push_str(")\n\n");
}

fn render_status_error(output: &mut String) {
    output.push_str(
        r#"// HTTPStatusError is an answer from the ingress that is not a synchronous
// result. It carries the status so a caller can tell a refusal apart from an
// answer another attempt could still get.
type HTTPStatusError struct {
	URL        string
	StatusCode int
	Body       string
	// RetryableHeader is the handler's Nexus-Request-Retryable answer, nil when
	// it sent none.
	RetryableHeader *bool
}

func (e *HTTPStatusError) Error() string {
	return fmt.Sprintf("%s failed (%d): %s", e.URL, e.StatusCode, e.Body)
}

// Retryable reports whether the same request could answer differently. The
// handler's Nexus-Request-Retryable header decides when it is set. Otherwise too
// many requests and the server-side failures are the endpoint's own transient
// conditions; every other status is about this request, and repeating it
// changes nothing.
func (e *HTTPStatusError) Retryable() bool {
	if e.RetryableHeader != nil {
		return *e.RetryableHeader
	}
	return e.StatusCode == http.StatusTooManyRequests || e.StatusCode >= 500
}

"#,
    );
}

fn render_long_poll_runtime(output: &mut String) {
    output.push_str(
        r#"const (
	longPollMinBackoff = 50 * time.Millisecond
	longPollMaxBackoff = 2 * time.Second
)

// longPollPause waits out a backoff without running past the read's own
// deadline, and gives up as soon as the context is done.
func longPollPause(ctx context.Context, backoff time.Duration, end time.Time) error {
	if remaining := time.Until(end); remaining < backoff {
		backoff = remaining
	}
	if backoff <= 0 {
		return nil
	}
	timer := time.NewTimer(backoff)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

func longPollNextBackoff(backoff time.Duration) time.Duration {
	doubled := backoff * 2
	if doubled > longPollMaxBackoff {
		return longPollMaxBackoff
	}
	return doubled
}

"#,
    );
}

fn render_service_client(output: &mut String, service: &ClientService) {
    let type_name = client_type_name(service);
    let options_name = format!("{type_name}Options");

    let mut summary = vec![format!(
        "{type_name} is an HTTP caller for the {} service.",
        service.wire_name
    )];
    if let Some(doc) = &service.doc {
        summary.push(doc.clone());
    }
    summary.push(
        "Every method posts to baseURL/<operation>, so baseURL is the service's address on \
         the Nexus HTTP ingress, up to but not including the operation name."
            .to_string(),
    );
    if service.deprecated {
        summary.push("Deprecated: this service is deprecated.".to_string());
    }
    render_wrapped_go_doc_comment(output, "", &summary.join(" "));
    output.push_str("type ");
    output.push_str(&type_name);
    output.push_str(" struct {\n");
    output.push_str("\tbaseURL string\n");
    output.push_str("\thttpClient *http.Client\n");
    output.push_str("\theader http.Header\n");
    output.push_str("}\n\n");

    render_wrapped_go_doc_comment(
        output,
        "",
        &format!("{options_name} configures a {type_name}. Every member is optional."),
    );
    output.push_str("type ");
    output.push_str(&options_name);
    output.push_str(" struct {\n");
    render_wrapped_go_doc_comment(
        output,
        "\t",
        "HTTPClient issues the requests. A nil value uses http.DefaultClient.",
    );
    output.push_str("\tHTTPClient *http.Client\n");
    render_wrapped_go_doc_comment(output, "\t", "Header is added to every request.");
    output.push_str("\tHeader http.Header\n");
    output.push_str("}\n\n");

    render_wrapped_go_doc_comment(
        output,
        "",
        &format!(
            "New{type_name} constructs a {type_name} posting to baseURL. A nil options is \
             the same as a zero one."
        ),
    );
    output.push_str("func New");
    output.push_str(&type_name);
    output.push_str("(baseURL string, options *");
    output.push_str(&options_name);
    output.push_str(") *");
    output.push_str(&type_name);
    output.push_str(" {\n");
    output.push_str("\tif options == nil {\n\t\toptions = &");
    output.push_str(&options_name);
    output.push_str("{}\n\t}\n");
    output.push_str("\thttpClient := options.HTTPClient\n");
    output.push_str("\tif httpClient == nil {\n\t\thttpClient = http.DefaultClient\n\t}\n");
    output.push_str("\treturn &");
    output.push_str(&type_name);
    output.push_str("{\n");
    output.push_str("\t\tbaseURL: strings.TrimRight(baseURL, \"/\"),\n");
    output.push_str("\t\thttpClient: httpClient,\n");
    output.push_str("\t\theader: options.Header,\n");
    output.push_str("\t}\n}\n\n");

    render_post_method(output, &type_name);
    if service
        .operations
        .iter()
        .any(|operation| operation.long_poll.is_some())
    {
        render_poll_budget_method(output, &type_name);
    }

    for operation in &service.operations {
        render_operation_method(output, &type_name, operation);
        if let Some(long_poll) = &operation.long_poll
            && let Some(input) = &operation.input
            && let Some(model_output) = &operation.output
        {
            render_long_poll_method(
                output,
                &type_name,
                operation,
                long_poll,
                input,
                model_output,
            );
        }
    }
}

fn render_poll_budget_method(output: &mut String, type_name: &str) {
    output.push_str("func (c *");
    output.push_str(type_name);
    output.push_str(
        r#") pollBudget(remaining time.Duration) time.Duration {
	if remaining < time.Millisecond {
		remaining = time.Millisecond
	}
	// Asking the endpoint to park for longer than the transport will wait is a
	// socket timeout dressed as a long poll, so the ask leaves the answer room
	// to come back inside the client's own deadline.
	if c.httpClient.Timeout > 0 {
		budget := c.httpClient.Timeout * 9 / 10
		if budget < time.Millisecond {
			budget = time.Millisecond
		}
		if remaining > budget {
			return budget
		}
	}
	return remaining
}

"#,
    );
}

fn render_post_method(output: &mut String, type_name: &str) {
    output.push_str("func (c *");
    output.push_str(type_name);
    output.push_str(
        r#") post(ctx context.Context, segment string, body []byte) ([]byte, error) {
	url := c.baseURL + "/" + segment
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	for name, values := range c.header {
		for _, value := range values {
			request.Header.Add(name, value)
		}
	}
	request.Header.Set("Content-Type", "application/json")
	// Tells the handler how long this caller waits, so it can cut a long poll
	// short instead of being cut off.
	timeout := c.httpClient.Timeout
	if deadline, ok := ctx.Deadline(); ok && (timeout <= 0 || time.Until(deadline) < timeout) {
		timeout = time.Until(deadline)
	}
	if timeout > 0 && request.Header.Get("Request-Timeout") == "" {
		request.Header.Set("Request-Timeout", fmt.Sprintf("%dms", timeout.Milliseconds()))
	}
	response, err := c.httpClient.Do(request)
	if err != nil {
		return nil, err
	}
	defer func() { _ = response.Body.Close() }()
	raw, err := io.ReadAll(response.Body)
	if err != nil {
		return nil, err
	}
	// A 201 means the handler started the operation asynchronously and answered
	// with a token, which this caller cannot read as the operation's result.
	if response.StatusCode != http.StatusOK && response.StatusCode != http.StatusNoContent {
		statusErr := &HTTPStatusError{URL: url, StatusCode: response.StatusCode, Body: string(raw)}
		if value := response.Header.Get("Nexus-Request-Retryable"); value != "" {
			retryable := strings.EqualFold(strings.TrimSpace(value), "true")
			statusErr.RetryableHeader = &retryable
		}
		return nil, statusErr
	}
	return raw, nil
}

"#,
    );
}

fn render_operation_method(output: &mut String, type_name: &str, operation: &ClientOperation) {
    let mut summary = vec![format!(
        "{} posts one {} call.",
        operation.name, operation.wire_name
    )];
    if let Some(doc) = &operation.doc {
        summary.push(doc.clone());
    }
    if operation.deprecated {
        summary.push("Deprecated: this operation is deprecated.".to_string());
    }
    render_wrapped_go_doc_comment(output, "", &summary.join(" "));

    output.push_str("func (c *");
    output.push_str(type_name);
    output.push_str(") ");
    output.push_str(&operation.name);
    output.push_str("(ctx context.Context");
    if let Some(input) = &operation.input {
        output.push_str(", request ");
        output.push_str(&input.type_name);
    }
    match &operation.output {
        Some(model_output) => {
            output.push_str(") (");
            output.push_str(&model_output.type_name);
            output.push_str(", error) {\n\tvar answer ");
            output.push_str(&model_output.type_name);
            output.push('\n');
        }
        None => output.push_str(") error {\n"),
    }
    let bail = |output: &mut String, indent: &str| {
        output.push_str(indent);
        if operation.output.is_some() {
            output.push_str("return answer, err\n");
        } else {
            output.push_str("return err\n");
        }
    };

    match &operation.input {
        Some(_) => {
            output.push_str("\tbody, err := json.Marshal(request)\n");
            output.push_str("\tif err != nil {\n");
            bail(output, "\t\t");
            output.push_str("\t}\n");
        }
        None => output.push_str("\tbody := []byte(\"{}\")\n"),
    }

    if operation.output.is_none() {
        output.push_str("\tif _, err := c.post(ctx, ");
        output.push_str(&literal(&operation.path_segment));
        output.push_str(", body); err != nil {\n\t\treturn err\n\t}\n\treturn nil\n}\n\n");
        return;
    }
    output.push_str("\traw, err := c.post(ctx, ");
    output.push_str(&literal(&operation.path_segment));
    output.push_str(", body)\n");
    output.push_str("\tif err != nil {\n");
    bail(output, "\t\t");
    output.push_str("\t}\n");
    // An operation that declares an output and answers with nothing has not
    // answered. Reading it as the all-defaults value would hand the caller a
    // value the endpoint never produced.
    output.push_str("\tif len(raw) == 0 {\n");
    output.push_str("\t\treturn answer, errors.New(");
    output.push_str(&literal(&format!(
        "{} answered with an empty body",
        operation.wire_name
    )));
    output.push_str(")\n\t}\n");
    output.push_str("\tif err := json.Unmarshal(raw, &answer); err != nil {\n");
    output.push_str("\t\treturn answer, err\n\t}\n");
    output.push_str("\treturn answer, nil\n}\n\n");
}

fn render_long_poll_method(
    output: &mut String,
    type_name: &str,
    operation: &ClientOperation,
    long_poll: &ClientLongPoll,
    input: &ClientModel,
    model_output: &ClientModel,
) {
    let method = long_poll_method_name(operation, long_poll, model_output);
    let wait_field = member_field(input.member(&long_poll.wait_member), &long_poll.wait_member);
    let result_field = member_field(
        model_output.member(&long_poll.result_member),
        &long_poll.result_member,
    );
    let done_test = long_poll.done_member.as_ref().map(|(done, required)| {
        let field = member_field(model_output.member(done), done);
        if *required {
            format!("answer.{field}")
        } else {
            format!("(answer.{field} != nil && *answer.{field})")
        }
    });
    let done_doc = long_poll
        .done_member
        .as_ref()
        .map(|(done, _)| {
            format!(
                ", or whose {} is true,",
                member_field(model_output.member(done), done)
            )
        })
        .unwrap_or_default();
    render_wrapped_go_doc_comment(
        output,
        "",
        &format!(
            "{method} calls {} until it answers with something, or until the deadline. Each \
             attempt sets {wait_field} from the budget left, so the endpoint parks for the \
             caller instead of the caller spinning. The first answer whose {result_field} is \
             non-empty{done_doc} is returned. When the deadline passes first, the last answer \
             is returned, which still carries the caller's resume position. The single-shot \
             method stays available for a caller that wants one attempt.",
            operation.name
        ),
    );
    output.push_str("func (c *");
    output.push_str(type_name);
    output.push_str(") ");
    output.push_str(&method);
    output.push_str("(ctx context.Context, request ");
    output.push_str(&input.type_name);
    output.push_str(", deadline time.Duration) (");
    output.push_str(&model_output.type_name);
    output.push_str(", error) {\n");
    output.push_str("\tend := time.Now().Add(deadline)\n");
    output.push_str("\tvar answer ");
    output.push_str(&model_output.type_name);
    output.push_str("\n\tbackoff := longPollMinBackoff\n");
    output.push_str("\tfor {\n");
    output.push_str("\t\twait := c.pollBudget(time.Until(end))\n");
    output.push_str("\t\tattempt := request\n");
    output.push_str("\t\tmilliseconds := wait.Milliseconds()\n");
    // The model refuses a wait outside the member's declared range.
    if let Some(maximum) = long_poll.wait_maximum {
        output.push_str(&format!(
            "\t\tif milliseconds > {maximum} {{\n\t\t\tmilliseconds = {maximum}\n\t\t}}\n"
        ));
    }
    if let Some(minimum) = long_poll.wait_minimum {
        output.push_str(&format!(
            "\t\tif milliseconds < {minimum} {{\n\t\t\tmilliseconds = {minimum}\n\t\t}}\n"
        ));
    }
    if long_poll.wait_maximum.is_some() || long_poll.wait_minimum.is_some() {
        output.push_str("\t\twait = time.Duration(milliseconds) * time.Millisecond\n");
    }
    // The loader keeps the wait member optional, so it is always a pointer.
    output.push_str("\t\tattempt.");
    output.push_str(&wait_field);
    output.push_str(" = &milliseconds\n");
    output.push_str("\t\tstarted := time.Now()\n");
    output.push_str("\t\tnext, err := c.");
    output.push_str(&operation.name);
    output.push_str("(ctx, attempt)\n");
    output.push_str("\t\tif err != nil {\n");
    output.push_str("\t\t\tvar status *HTTPStatusError\n");
    output.push_str(
        "\t\t\tif !errors.As(err, &status) || !status.Retryable() || !time.Now().Before(end) {\n",
    );
    output.push_str("\t\t\t\treturn answer, err\n\t\t\t}\n");
    output.push_str("\t\t\tif paused := longPollPause(ctx, backoff, end); paused != nil {\n");
    output.push_str("\t\t\t\treturn answer, paused\n\t\t\t}\n");
    output.push_str("\t\t\tbackoff = longPollNextBackoff(backoff)\n");
    output.push_str("\t\t\tcontinue\n\t\t}\n");
    output.push_str("\t\tanswer = next\n");
    output.push_str("\t\tif len(answer.");
    output.push_str(&result_field);
    output.push_str(") > 0");
    if let Some(done) = &done_test {
        output.push_str(" || ");
        output.push_str(done);
    }
    output.push_str(" {\n\t\t\treturn answer, nil\n\t\t}\n");
    output.push_str("\t\tif !time.Now().Before(end) {\n\t\t\treturn answer, nil\n\t\t}\n");
    // An endpoint that answers empty without parking for the wait it was given
    // would otherwise be asked again as fast as the network allows.
    output.push_str("\t\tif time.Since(started) < wait/2 {\n");
    output.push_str("\t\t\tif paused := longPollPause(ctx, backoff, end); paused != nil {\n");
    output.push_str("\t\t\t\treturn answer, paused\n\t\t\t}\n");
    output.push_str("\t\t\tbackoff = longPollNextBackoff(backoff)\n");
    output.push_str("\t\t} else {\n\t\t\tbackoff = longPollMinBackoff\n\t\t}\n");
    output.push_str("\t}\n}\n\n");
}
