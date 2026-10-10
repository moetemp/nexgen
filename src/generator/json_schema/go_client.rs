//! The generated Go HTTP caller (`--client`).
//!
//! This is a different caller from the `{Service}Client` the NativeApi surface
//! emits: that one starts a Nexus operation from inside a workflow, while this
//! one is for a process outside the worker posting to the Nexus HTTP ingress.
//! It depends on the standard library only, because a stream producer is often
//! not a Temporal worker at all.

use crate::generator::go::{
    GENERATED_HEADER, render_go_doc_comment as render_wrapped_go_doc_comment,
};
use crate::generator::json_schema::client::{ClientOperation, ClientPlan, ClientService};

/// The file the caller is emitted to, in the same flat package as the models.
pub(in crate::generator) const CLIENT_FILE: &str = "client.go";

fn literal(value: &str) -> String {
    serde_json::to_string(value).expect("a Rust string is always JSON-serializable")
}

fn client_type_name(service: &ClientService) -> String {
    format!("{}HTTPClient", service.name)
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
    for service in &plan.services {
        render_service_client(&mut output, service);
    }
    Some(output)
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
    for operation in &service.operations {
        render_operation_method(output, &type_name, operation);
    }
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
