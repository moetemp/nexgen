// Generation with no API target writes definitions only. A handler that implements a service
// needs only those, and it shares its WIT with the callers, so resources and sources are common.
// The default build has no `--targets`, so it always generates this way.

use std::fs;

use nexgen::language::Language;
use nexgen::{GenerateRequest, generate_to_file};

const WIT: &str = r#"
package temporal:definitions@1.0.0;

world system { export jobs; }

/// @nexus.endpoint "jobs"
interface jobs {
  resource job {
    constructor(job-id: string);

    restart: func();
  }

  record restart-request {
    job-id: string,
    /// @nexus.source
    ///   python="namespace()"
    ///   typescript="namespace()"
    ///   go="Namespace(ctx)"
    ///   dotnet="Support.Namespace()"
    namespace: string,
  }

  restart: func(request: restart-request);
}
"#;

#[test]
fn a_resource_and_a_sourced_field_generate_with_no_api_target() {
    let temp_dir = tempfile::tempdir().unwrap();
    let wit_path = temp_dir.path().join("jobs.wit");
    fs::write(&wit_path, WIT).unwrap();
    for (language, support_package) in [
        (Language::Python, "temporal_support"),
        (Language::TypeScript, "./support"),
        (Language::Go, "example.com/support"),
        (Language::Dotnet, "Nexgen.Support"),
    ] {
        generate_to_file(&GenerateRequest {
            config: Default::default(),
            language,
            input_paths: vec![wit_path.clone()],
            support_package: Some(support_package.into()),
            descriptor_paths: Vec::new(),
            output_path: temp_dir.path().join(format!("{language:?}")),
            format: false,
            java_package_name: None,
            ts_date_time_types: Default::default(),
            dotnet_namespace: None,
        })
        .unwrap_or_else(|error| panic!("{language:?}: {error}"));
    }
}
