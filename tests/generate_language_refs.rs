//! `$<lang>Ref` on an operation's input or output: the input names an existing
//! type per language instead of declaring a shape, as the upstream
//! nexus-rpc-gen schema allows. Python output imports the `$pythonRef` type,
//! and the Temporal api files that use the form generate as they are.

mod toolchain;

use std::fs;
use std::path::{Path, PathBuf};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};
use toolchain::{Workspace, python_runtime_command, run};

/// The api files that use the form. See `tests/fixtures/README.md`.
const NOTIFICATION_SERVICE: &str = "tests/fixtures/api/notification-service.nexusrpc.yaml";
const PROTO_MODELS: &str = "tests/fixtures/api/temporal-proto-models-nexusrpc.yaml";
const WORKER_SERVICE: &str = "tests/fixtures/api/nexus-rpc/temporal-proto-models-nexusrpc.yaml";

/// Upstream form with types the released Python SDK carries, so the toolchain
/// test can import them.
const CONTRACT: &str = r#"
nexusrpc: 1.0.0
services:
  example.payloads.v1.PayloadService:
    description: Echoes payloads.
    operations:
      Echo:
        description: Echoes the payloads.
        input:
          $goRef: go.temporal.io/api/common/v1.Payloads
          $pythonRef: temporalio.api.common.v1.Payloads
        output:
          $goRef: go.temporal.io/api/common/v1.Payloads
          $pythonRef: temporalio.api.common.v1.Payloads
      Signal:
        input:
          $pythonRef: temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest
        output:
          $pythonRef: temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse
"#;

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn generate(input: &Path, output_path: &Path, language: Language) -> Result<(), String> {
    generate_with(input, output_path, language, NexgenConfig::default())
}

fn generate_with(
    input: &Path,
    output_path: &Path,
    language: Language,
    config: NexgenConfig,
) -> Result<(), String> {
    generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            ..config
        },
        language,
        input_paths: vec![input.to_path_buf()],
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.to_path_buf(),
        format: false,
        java_package_name: (language == Language::Java).then(|| "refs".to_string()),
        ts_date_time_types: Default::default(),
    })
    .map_err(|error| error.to_string())
}

fn write_contract(workspace: &Workspace, contract: &str) -> PathBuf {
    let path = workspace.root().join("contract.nexusrpc.yaml");
    fs::write(&path, contract).expect("write the contract");
    path
}

fn python_services(input: &Path, tag: &str) -> String {
    let workspace = Workspace::new(tag);
    let output_path = workspace.root().join("refs");
    generate(input, &output_path, Language::Python)
        .unwrap_or_else(|error| panic!("Python should generate: {error}"));
    fs::read_to_string(output_path.join("services.py")).expect("read services.py")
}

fn refusal(contract: &str, language: Language, tag: &str) -> String {
    let workspace = Workspace::new(tag);
    let input = write_contract(&workspace, contract);
    generate(&input, &workspace.root().join("refs"), language)
        .expect_err("the contract should be refused")
}

#[test]
fn the_notification_service_names_its_proto_messages() {
    let services = python_services(&fixture(NOTIFICATION_SERVICE), "refs-notification");
    assert!(
        services.contains(
            "@service(name=\"temporal.api.notificationservice.v1.NotificationService\")\n\
             class NotificationService:\n"
        ),
        "{services}"
    );
    assert!(
        services.contains("import temporalio.api.notificationservice.v1\n"),
        "{services}"
    );
    for (attr, wire, message) in [
        ("on_complete", "OnComplete", "OnComplete"),
        ("on_progress", "OnProgress", "OnProgress"),
    ] {
        let expected = format!(
            "    {attr}: Operation[\n        \
             temporalio.api.notificationservice.v1.{message}Request,\n        \
             temporalio.api.notificationservice.v1.{message}Response,\n    \
             ] = Operation(name=\"{wire}\")\n"
        );
        assert!(
            services.contains(&expected),
            "{expected}\nnot in\n{services}"
        );
    }
}

#[test]
fn a_named_type_emits_no_model() {
    let workspace = Workspace::new("refs-no-model");
    let output_path = workspace.root().join("refs");
    generate(
        &fixture(NOTIFICATION_SERVICE),
        &output_path,
        Language::Python,
    )
    .expect("Python should generate");
    let models = fs::read_to_string(output_path.join("models.py")).unwrap_or_default();
    assert!(!models.contains("OnCompleteInput"), "{models}");
    assert!(!models.contains("class "), "{models}");
}

#[test]
fn the_api_proto_model_files_generate() {
    let services = python_services(&fixture(PROTO_MODELS), "refs-proto-models");
    assert!(
        services.contains(
            "    signal_with_start_workflow_execution: Operation[\n        \
             temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest,\n"
        ),
        "{services}"
    );
    let services = python_services(&fixture(WORKER_SERVICE), "refs-worker-service");
    assert!(
        services.contains("] = Operation(name=\"ExecuteCommands\")\n"),
        "{services}"
    );
    assert!(
        services.contains("import temporalio.api.nexusservices.workerservice.v1\n"),
        "{services}"
    );
}

/// The released SDK imports the generated definition, and a handler can
/// implement it.
#[test]
fn the_generated_definition_serves_the_named_types() {
    const DRIVER: &str = r#"
import importlib
import sys

import nexusrpc
import temporalio.api.common.v1
import temporalio.api.workflowservice.v1

sys.path.insert(0, sys.argv[1])
services = importlib.import_module("refs.services")
definition = nexusrpc.get_service_definition(services.PayloadService)
assert definition.name == "example.payloads.v1.PayloadService", definition.name
echo = definition.operation_definitions["Echo"]
assert echo.method_name == "echo", echo.method_name
assert echo.input_type is temporalio.api.common.v1.Payloads, echo.input_type
assert echo.output_type is temporalio.api.common.v1.Payloads, echo.output_type
signal = definition.operation_definitions["Signal"]
assert (
    signal.input_type
    is temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest
)


@nexusrpc.handler.service_handler(service=services.PayloadService)
class Handler:
    @nexusrpc.handler.sync_operation
    async def echo(
        self, ctx: nexusrpc.handler.StartOperationContext, input: temporalio.api.common.v1.Payloads
    ) -> temporalio.api.common.v1.Payloads:
        return input

    @nexusrpc.handler.sync_operation
    async def signal(
        self,
        ctx: nexusrpc.handler.StartOperationContext,
        input: temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionRequest,
    ) -> temporalio.api.workflowservice.v1.SignalWithStartWorkflowExecutionResponse:
        raise NotImplementedError


print("ok")
"#;
    let workspace = Workspace::new("refs-toolchain");
    let input = write_contract(&workspace, CONTRACT);
    let package = workspace.root().join("python").join("refs");
    fs::create_dir_all(package.parent().expect("a package parent")).expect("create the parent");
    generate(&input, &package, Language::Python).expect("generate the bindings");
    let driver = workspace.root().join("drive_refs.py");
    fs::write(&driver, DRIVER).expect("write the driver");
    let mut python = python_runtime_command().expect("a Python with the SDK");
    let output = run(python
        .arg(&driver)
        .arg(package.parent().expect("a package parent")))
    .expect_ok("drive the definition");
    assert!(output.contains("ok"), "{output}");
}

#[test]
fn a_contract_without_a_python_ref_is_refused_for_python() {
    let contract = CONTRACT.replace(
        "          $pythonRef: temporalio.api.common.v1.Payloads\n        output:",
        "        output:",
    );
    let error = refusal(&contract, Language::Python, "refs-no-python");
    assert!(error.contains("`$pythonRef`"), "{error}");
    assert!(error.contains("`$goRef`"), "{error}");
    assert!(error.contains("echo"), "{error}");
}

#[test]
fn a_named_type_is_refused_for_a_target_that_does_not_resolve_it() {
    for language in [Language::Go, Language::TypeScript, Language::Java] {
        let error = refusal(CONTRACT, language, &format!("refs-target-{language}"));
        assert!(
            error.contains("only for Python output"),
            "{language}: {error}"
        );
    }
}

#[test]
fn a_named_type_with_a_shape_is_refused() {
    let contract = CONTRACT.replacen(
        "          $goRef: go.temporal.io/api/common/v1.Payloads\n",
        "          $goRef: go.temporal.io/api/common/v1.Payloads\n          type: object\n",
        1,
    );
    let error = refusal(&contract, Language::Python, "refs-shape");
    assert!(error.contains("`type`"), "{error}");
    assert!(error.contains("existing type"), "{error}");
}

#[test]
fn a_ref_keyword_outside_an_operation_is_refused_with_a_remedy() {
    let contract = format!(
        "{CONTRACT}$defs:\n  Holder:\n    type: object\n    properties:\n      \
         payloads:\n        $pythonRef: temporalio.api.common.v1.Payloads\n"
    );
    let error = refusal(&contract, Language::Python, "refs-in-defs");
    assert!(error.contains("`$pythonRef`"), "{error}");
    assert!(error.contains("operation `input` or `output`"), "{error}");
}

#[test]
fn a_python_ref_must_be_a_qualified_name() {
    for bad in [
        "Payloads",
        "temporalio.api..Payloads",
        "1temporalio.Payloads",
    ] {
        let contract = CONTRACT.replacen(
            "$pythonRef: temporalio.api.common.v1.Payloads",
            &format!("$pythonRef: \"{bad}\""),
            1,
        );
        let error = refusal(&contract, Language::Python, "refs-unqualified");
        assert!(
            error.contains("qualified Python type name"),
            "{bad}: {error}"
        );
    }
}

#[test]
fn a_ref_value_must_be_a_string() {
    let contract = CONTRACT.replacen(
        "$goRef: go.temporal.io/api/common/v1.Payloads",
        "$goRef: 7",
        1,
    );
    let error = refusal(&contract, Language::Python, "refs-ref-value");
    assert!(
        error.contains("`$goRef`") && error.contains("string"),
        "{error}"
    );
}

#[test]
fn two_operations_that_fold_to_one_name_are_refused() {
    let contract = CONTRACT.replace(
        "      Signal:\n",
        "      echo:\n        input: { type: object }\n      Signal:\n",
    );
    let error = refusal(&contract, Language::Python, "refs-fold-operation");
    assert!(
        error.contains("`Echo`") && error.contains("`echo`"),
        "{error}"
    );
}

#[test]
fn two_services_that_fold_to_one_name_are_refused() {
    let contract = format!(
        "{CONTRACT}  other.v1.PayloadService:\n    operations:\n      \
         ping:\n        input: {{ type: object }}\n"
    );
    let error = refusal(&contract, Language::Python, "refs-fold-service");
    assert!(
        error.contains("`example.payloads.v1.PayloadService`")
            && error.contains("`other.v1.PayloadService`"),
        "{error}"
    );
}

/// Both write JSON transfer forms, which a named type doesn't have.
#[test]
fn the_json_caller_and_forwarder_refuse_a_named_type() {
    for (flag, config) in [
        (
            "client",
            NexgenConfig {
                client: true,
                ..Default::default()
            },
        ),
        (
            "forwarder",
            NexgenConfig {
                forwarder: true,
                ..Default::default()
            },
        ),
    ] {
        let workspace = Workspace::new(&format!("refs-{flag}"));
        let error = generate_with(
            &fixture(NOTIFICATION_SERVICE),
            &workspace.root().join("refs"),
            Language::Python,
            config,
        )
        .expect_err("the flag should be refused");
        assert!(
            error.contains("`OnComplete`") && error.contains("`--client`"),
            "{flag}: {error}"
        );
    }
}

#[test]
fn an_annotation_that_reads_members_is_refused_on_a_named_operation() {
    let contract = CONTRACT.replace(
        "      Echo:\n",
        "      Echo:\n        x-nexus-long-poll: { wait-field: waitMs, result-field: items }\n",
    );
    let error = refusal(&contract, Language::Python, "refs-long-poll");
    assert!(error.contains("`x-nexus-long-poll`"), "{error}");
    assert!(error.contains("existing type"), "{error}");
}
