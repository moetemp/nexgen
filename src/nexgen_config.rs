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
    /// Parses `PROTO_PREFIX=MODULE`, the form of the CLI flag and of `examples.toml`.
    pub fn parse(value: &str) -> Option<Self> {
        let (proto_prefix, module) = value.split_once('=')?;
        (!proto_prefix.is_empty() && !module.is_empty()).then(|| Self {
            proto_prefix: proto_prefix.to_string(),
            module: module.to_string(),
        })
    }
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
    use super::{NexgenConfig, current, scope};
    use crate::generator::GenerationMode;

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
