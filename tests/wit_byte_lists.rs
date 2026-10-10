//! A WIT `list<u8>` outside a proto-backed record keeps the list wire form.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use nexgen::language::Language;
use nexgen::{GenerateRequest, generate_to_file};

/// The reviewer's repro: a plain WIT record with no `@nexus.proto`.
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

fn generate(language: Language, support_package: &str) -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir: PathBuf =
        std::env::temp_dir().join(format!("nexgen-wit-byte-lists-{language:?}-{unique}"));
    fs::create_dir_all(&temp_dir).unwrap();
    let input_path = temp_dir.join("store.wit");
    fs::write(&input_path, PLAIN_RECORD_WIT).unwrap();
    let output_path = temp_dir.join("generated");
    generate_to_file(&GenerateRequest {
        config: Default::default(),
        language,
        input_paths: vec![input_path],
        support_package: Some(support_package.to_string()),
        descriptor_paths: Vec::new(),
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
fn go_keeps_a_plain_wit_byte_list_as_a_list_of_integers() {
    let models = generate(Language::Go, "example.com/nexgen/support");
    assert!(models.contains("Data []int32"), "{models}");
    assert!(!models.contains("[]byte"), "{models}");
}

#[test]
fn typescript_keeps_a_plain_wit_byte_list_as_a_list_of_integers() {
    let models = generate(Language::TypeScript, "./support");
    assert!(models.contains("data?: number[]"), "{models}");
    assert!(!models.contains("Uint8Array"), "{models}");
}
