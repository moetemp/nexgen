//! The opaque token types `x-nexus-cursor` declares.
//!
//! The committed `samples/schemas/streams.nexusrpc.yaml` output is the golden
//! for one file; these pin the decisions it does not show: two input files
//! naming one token type, the wire members it leaves alone, and the targets
//! that declare nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::language::Language;
use nexgen::nexgen_config::NexgenConfig;
use nexgen::{GenerateRequest, generate_to_file};

const ALPHA: &str = r##"
nexusrpc: "1.0.0"
services:
  AlphaService:
    operations:
      read:
        input: { $ref: "#/$defs/ReadInput" }
$defs:
  ReadInput:
    type: object
    properties:
      afterToken: { type: string, x-nexus-cursor: StreamCursor }
      pageToken: { type: string, x-nexus-cursor: PageCursor }
    required: [pageToken]
    additionalProperties: false
"##;

const BETA: &str = r##"
nexusrpc: "1.0.0"
services:
  BetaService:
    operations:
      write:
        input: { $ref: "#/$defs/WriteInput" }
$defs:
  WriteInput:
    type: object
    properties:
      cursor: { type: string, x-nexus-cursor: StreamCursor }
    additionalProperties: false
"##;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_output_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let ordinal = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nexgen-{label}-{nanos}-{ordinal}"))
}

/// Generates the given input files into one output and answers with every
/// emitted file, keyed by its path below the output root.
fn generate(
    language: Language,
    contracts: &[(&str, &str)],
    label: &str,
) -> (PathBuf, Vec<(String, String)>) {
    let temp_dir = unique_output_path(label);
    let input_dir = temp_dir.join("input");
    fs::create_dir_all(&input_dir).unwrap();
    let mut input_paths = Vec::new();
    for (name, contract) in contracts {
        let input_path = input_dir.join(name);
        fs::write(&input_path, contract).unwrap();
        input_paths.push(input_path);
    }
    let output_path = temp_dir.join("streams");
    generate_to_file(&GenerateRequest {
        config: NexgenConfig::default(),
        language,
        input_paths,
        support_package: None,
        descriptor_paths: Vec::new(),
        output_path: output_path.clone(),
        format: false,
        java_package_name: (language == Language::Java).then(|| "streams".to_string()),
        ts_date_time_types: Default::default(),
    })
    .unwrap_or_else(|error| panic!("{language:?} should generate: {error}"));
    let mut files = Vec::new();
    collect_files(&output_path, &output_path, &mut files);
    files.sort();
    (temp_dir, files)
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            files.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

fn file<'a>(files: &'a [(String, String)], name: &str) -> &'a str {
    files
        .iter()
        .find(|(file_name, _)| file_name == name)
        .map(|(_, contents)| contents.as_str())
        .unwrap_or_else(|| {
            panic!(
                "{name} should be generated; got {:?}",
                files.iter().map(|(name, _)| name).collect::<Vec<_>>()
            )
        })
}

fn declarations(files: &[(String, String)], needle: &str) -> Vec<String> {
    files
        .iter()
        .filter(|(_, contents)| contents.contains(needle))
        .map(|(name, contents)| format!("{name}: {}", contents.matches(needle).count()))
        .collect()
}

#[test]
fn python_declares_each_token_type_once_in_the_shared_runtime() {
    let (temp_dir, files) = generate(
        Language::Python,
        &[("alpha.nexusrpc.yaml", ALPHA), ("beta.nexusrpc.yaml", BETA)],
        "cursor-python-pair",
    );
    // Two modules naming one type share one declaration, so a token read from
    // one is accepted by the other.
    assert_eq!(
        declarations(
            &files,
            "StreamCursor = typing.NewType(\"StreamCursor\", str)"
        ),
        vec!["_definitions.py: 1".to_string()]
    );
    let runtime = file(&files, "_definitions.py");
    assert!(
        runtime.contains("PageCursor = typing.NewType(\"PageCursor\", str)"),
        "{runtime}"
    );
    assert!(runtime.contains("never parsed"), "{runtime}");
    assert!(runtime.contains("\"StreamCursor\","), "{runtime}");
    let root = file(&files, "__init__.py");
    assert!(root.contains("    StreamCursor,"), "{root}");
    assert!(root.contains("\"PageCursor\","), "{root}");
    // The wire members stay strings: the token type is for a caller's own
    // variables, and the JSON on the wire is unchanged.
    let models = file(&files, "alpha/models.py");
    assert!(models.contains("after_token: str | None"), "{models}");
    assert!(models.contains("page_token: str"), "{models}");
    assert!(!models.contains("StreamCursor"), "{models}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn go_declares_each_token_type_once_in_definitions() {
    let (temp_dir, files) = generate(
        Language::Go,
        &[("alpha.nexusrpc.yaml", ALPHA), ("beta.nexusrpc.yaml", BETA)],
        "cursor-go-pair",
    );
    assert_eq!(
        declarations(&files, "type StreamCursor string"),
        vec!["definitions.go: 1".to_string()]
    );
    let definitions = file(&files, "definitions.go");
    assert!(
        definitions.contains("type PageCursor string"),
        "{definitions}"
    );
    assert!(
        definitions.contains("// StreamCursor is an opaque resume token issued by the endpoint."),
        "{definitions}"
    );
    let alpha = file(&files, "alpha.go");
    assert!(alpha.contains("AfterToken *string"), "{alpha}");
    assert!(alpha.contains("PageToken string"), "{alpha}");
    fs::remove_dir_all(temp_dir).unwrap();
}

#[test]
fn the_other_targets_declare_no_token_type() {
    for language in [Language::TypeScript, Language::Java] {
        let (temp_dir, files) = generate(
            language,
            &[("alpha.nexusrpc.yaml", ALPHA)],
            "cursor-other-targets",
        );
        for (name, contents) in &files {
            assert!(
                !contents.contains("StreamCursor") && !contents.contains("x-nexus-"),
                "{language:?} {name} acted on the keyword"
            );
        }
        fs::remove_dir_all(temp_dir).unwrap();
    }
}
