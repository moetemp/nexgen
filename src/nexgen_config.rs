//! Scoped generation settings shared across the compiler.
//!
//! Configuration is thread-local so concurrent in-process callers cannot affect
//! one another. Scopes nest and restore the prior configuration during unwinding.

use std::cell::RefCell;

use crate::generator::GenerationMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexgenConfig {
    pub mode: GenerationMode,
    pub system_nexus: bool,
    /// Python modules for protos that `temporalio.api` doesn't have yet, such as an API that isn't
    /// released. Without an entry, a `temporal/...` proto file is a `temporalio...` module.
    pub python_proto_modules: Vec<ProtoModule>,
}

/// A proto file path prefix, such as `temporal/api/stream/` or one `.proto` file, and the Python
/// module path that it maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoModule {
    pub proto_prefix: String,
    pub module: String,
}

impl ProtoModule {
    /// Parses `PROTO_PREFIX=MODULE`, the form of the CLI flag and of `examples.toml`. `MODULE` must
    /// be a dotted Python name, such as `protos.temporal.api.stream`.
    pub fn parse(value: &str) -> Option<Self> {
        let (proto_prefix, module) = value.split_once('=')?;
        (!proto_prefix.is_empty() && module.split('.').all(is_python_identifier)).then(|| Self {
            proto_prefix: proto_prefix.to_string(),
            module: module.to_string(),
        })
    }
}

fn is_python_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|rest| rest == '_' || rest.is_ascii_alphanumeric())
}

impl Default for NexgenConfig {
    fn default() -> Self {
        Self {
            mode: GenerationMode::DefinitionsOnly,
            system_nexus: false,
            python_proto_modules: Vec::new(),
        }
    }
}

thread_local! {
    static CONFIGS: RefCell<Vec<NexgenConfig>> = RefCell::new(Vec::new());
}

pub fn current() -> NexgenConfig {
    CONFIGS.with(|configs| configs.borrow().last().cloned().unwrap_or_default())
}

pub fn scope(config: NexgenConfig) -> NexgenConfigScope {
    CONFIGS.with(|configs| configs.borrow_mut().push(config));
    NexgenConfigScope
}

pub struct NexgenConfigScope;

impl Drop for NexgenConfigScope {
    fn drop(&mut self) {
        CONFIGS.with(|configs| {
            configs
                .borrow_mut()
                .pop()
                .expect("NexGen configuration scope must be present");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{NexgenConfig, ProtoModule, current, scope};
    use crate::generator::GenerationMode;

    #[test]
    fn a_proto_module_must_be_a_dotted_python_name() {
        let parsed = ProtoModule::parse("temporal/api/stream/=protos.temporal_api.stream").unwrap();
        assert_eq!(parsed.proto_prefix, "temporal/api/stream/");
        assert_eq!(parsed.module, "protos.temporal_api.stream");
        for value in [
            "temporal/api/stream/=",
            "temporal/api/stream/=protos.temporal-api",
            "temporal/api/stream/=protos..stream",
            "temporal/api/stream/=.protos",
            "temporal/api/stream/=1protos",
            "temporal/api/stream/=protos/stream",
            "=protos",
            "protos",
        ] {
            assert_eq!(ProtoModule::parse(value), None, "{value}");
        }
    }

    #[test]
    fn restores_nested_configuration_after_unwinding() {
        let outer = NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            system_nexus: true,
            python_proto_modules: Vec::new(),
        };
        {
            let _outer_scope = scope(outer.clone());
            assert_eq!(current(), outer);
            let result = std::panic::catch_unwind(|| {
                let _inner_scope = scope(NexgenConfig::default());
                panic!("test unwind");
            });
            assert!(result.is_err());
            assert_eq!(current(), outer);
        }
        assert_eq!(current(), NexgenConfig::default());
    }

    #[test]
    fn is_isolated_per_thread() {
        let config = NexgenConfig {
            mode: GenerationMode::DefinitionsOnly,
            system_nexus: true,
            python_proto_modules: Vec::new(),
        };
        {
            let _scope = scope(config.clone());
            assert_eq!(current(), config);
            assert_eq!(
                std::thread::spawn(current).join().unwrap(),
                NexgenConfig::default()
            );
        }
    }
}
