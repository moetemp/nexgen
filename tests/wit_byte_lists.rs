//! A WIT `list<u8>` field of a `@nexus.proto` record generates as proto `bytes`.
//! A `list<u8>` field of a record without a proto generates as a list of integers.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::language::Language;
use nexgen::{GenerateRequest, generate_to_file};

/// A WIT record with no `@nexus.proto`.
const PLAIN_RECORD_WIT: &str = r#"package acme:store@1.0.0;

world store {
  export store-service;
}

interface store-service {
  record put-request {
    key: string,
    data: list<u8>,
    ints: list<s32>,
  }

  record put-response {
    key: string,
  }

  put: func(request: put-request) -> put-response;
}
"#;

/// `add-rpc` scaffolds proto `bytes` as `list<u8>`.
const PROTO_BYTES_WIT: &str = r#"package temporal:nexus@1.0.0;

world system {
  export callbacks;
}

interface callbacks {
  /// @nexus.proto "temporal.api.common.v1.Callback.Internal"
  record callback-internal {
    data: list<u8>,
  }
}
"#;

fn generate(language: Language, support_package: &str) -> String {
    generate_from(PLAIN_RECORD_WIT, Vec::new(), language, support_package)
}

fn generate_from(
    wit: &str,
    descriptor_paths: Vec<PathBuf>,
    language: Language,
    support_package: &str,
) -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir: PathBuf =
        std::env::temp_dir().join(format!("nexgen-wit-byte-lists-{language:?}-{unique}"));
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("store.wit");
    fs::write(&input_path, wit).unwrap();
    let output_path = temp_dir.join("generated");
    generate_to_file(&GenerateRequest {
        config: Default::default(),
        language,
        input_paths: vec![input_path],
        support_package: Some(support_package.to_string()),
        descriptor_paths,
        output_path: output_path.clone(),
        format: false,
        java_package_name: None,
        ts_date_time_types: Default::default(),
    })
    .unwrap();
    let mut rendered = String::new();
    read_all(&output_path, &mut rendered);
    fs::remove_dir_all(temp_dir).unwrap();
    rendered
}

fn read_all(dir: &std::path::Path, out: &mut String) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            read_all(&path, out);
        } else {
            out.push_str(&fs::read_to_string(&path).unwrap());
        }
    }
}

#[test]
fn python_keeps_a_plain_wit_byte_list_as_a_list_of_integers() {
    let models = generate(Language::Python, "temporal_support");
    assert!(models.contains("    data: list[int]"), "{models}");
    assert!(!models.contains("bytes"), "{models}");
}

#[test]
fn python_reads_a_proto_backed_wit_byte_list_as_bytes() {
    let descriptors = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("advanced/samples/descriptors/temporal_api.bin");
    let models = generate_from(
        PROTO_BYTES_WIT,
        vec![descriptors],
        Language::Python,
        "temporal_support",
    );
    assert!(models.contains("    data: bytes\n"), "{models}");
}
