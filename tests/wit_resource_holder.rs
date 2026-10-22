//! A result record that holds a resource, on the `workflow-run` sample.
#![cfg(feature = "advanced")]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::generator::GenerationMode;
use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};

static OUTPUT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generates the sample for `language` and returns the text of `file`.
fn generate(language: Language, file: &str) -> Result<String, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = OUTPUT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let output_path =
        std::env::temp_dir().join(format!("nexgen-holder-{language:?}-{unique}-{counter}"));
    let result = generate_to_file(&GenerateRequest {
        config: NexgenConfig {
            mode: GenerationMode::NativeApi,
            system_nexus: true,
            ..Default::default()
        },
        language,
        input_paths: vec![
            root.join("advanced/samples/inputs/workflow-run.wit"),
            root.join("advanced/samples/inputs/deps"),
        ],
        support_package: Some(
            match language {
                Language::Dotnet => "Acme.Support",
                _ => "acme_support",
            }
            .to_string(),
        ),
        descriptor_paths: vec![root.join("advanced/samples/descriptors/temporal_api.bin")],
        output_path: output_path.clone(),
        format: false,
        java_package_name: Some("generated".to_string()),
        ts_date_time_types: Default::default(),
    })
    .map(|()| fs::read_to_string(output_path.join(file)).unwrap())
    .map_err(|error| error.to_string());
    let _ = fs::remove_dir_all(output_path);
    result
}

#[test]
fn python_result_record_holds_the_resource() {
    let start = generate(Language::Python, "operations/start_run.py").unwrap();
    assert!(
        start.contains(
            "    return StartRunResult(\n        run=WorkflowRun(\n            \
             workflow_id=request.workflow_id,\n            run_id=result.run_id or None,\n        \
             ),\n        started=result.started,\n    )\n"
        ),
        "{start}"
    );
}

#[test]
fn typescript_result_record_holds_the_resource() {
    let start = generate(Language::TypeScript, "operations/start-run.ts").unwrap();
    assert!(
        start.contains(
            "  return { run: new WorkflowRun(request.workflowId, result.runId ?? undefined), \
             started: result.started };\n"
        ),
        "{start}"
    );
}

#[test]
fn dotnet_result_record_holds_the_resource() {
    let operations = generate(Language::Dotnet, "Operations.cs").unwrap();
    assert!(
        operations.contains(
            "return new StartRunResult(new WorkflowRun(request.WorkflowId, \
             string.IsNullOrEmpty(result.RunId) ? null : result.RunId)) { Started = result.Started };"
        ),
        "{operations}"
    );
}

#[test]
fn go_result_record_holds_the_resource() {
    let service = generate(Language::Go, "workflowrunservice.go").unwrap();
    assert!(
        service.contains(
            "\t\tvalue := StartRunResult{Run: NewWorkflowRun(request.WorkflowID, \
             result.GetRunId()), Started: heldStarted}\n"
        ),
        "{service}"
    );
}
