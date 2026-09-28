//! Which System 1 engines run on which source. Routing is app composition: engines never know
//! which stream they are attached to, and `s2w_sources::registry` maps URIs to transports, not
//! payloads to engines.

use s2w_model::SourceId;
use s2w_system1::{Engine, JsonClaimsEngine, LocalEmbeddingsEngine, WikimediaPageChangeEngine};

/// Which source ids a registered engine runs on. Names are compared as text, so routing a name
/// no source ever produces is not an error; it just never matches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Exactly this source id.
    Exact(&'static str),
    /// Every source id starting with this prefix. Include the separator (`"wikipedia."`) so
    /// the prefix cannot match a longer sibling name (`"wikipediafoo"`).
    Prefix(&'static str),
}

impl Route {
    fn matches(&self, source: &SourceId) -> bool {
        match self {
            Self::Exact(id) => source.as_str() == *id,
            Self::Prefix(prefix) => source.as_str().starts_with(prefix),
        }
    }
}

/// Why [`EngineRegistry::register`] refused an engine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// An engine of this name is already registered at another version. Stored verdicts are
    /// keyed by `(position, engine, version)`, so one name must mean one version per registry.
    #[error("engine '{name}' is registered at version {registered}; version {rejected} refused")]
    VersionConflict {
        /// The engine name.
        name: &'static str,
        /// The version already registered.
        registered: u32,
        /// The version refused.
        rejected: u32,
    },
}

/// Engines keyed by route, in registration order.
///
/// Several engines may match one source; the bridge runs all of them in registration order,
/// so the resulting timeline is a deterministic function of the log and this registry. One
/// engine name runs at most once per event, even when it is registered on overlapping routes.
#[derive(Default)]
pub struct EngineRegistry {
    routes: Vec<(Route, Box<dyn Engine>)>,
}

impl EngineRegistry {
    /// An empty registry: every source is unrouted.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The first-slice routing: Wikimedia page changes to the rules engine and the local
    /// embeddings engine (decision 0013), `stdin` to the JSON-claims engine.
    #[must_use]
    pub fn with_defaults() -> Self {
        // Three distinct names, so `register`'s version check cannot fire.
        Self {
            routes: vec![
                (
                    Route::Prefix("wikipedia."),
                    Box::new(WikimediaPageChangeEngine),
                ),
                (
                    Route::Prefix("wikipedia."),
                    Box::new(LocalEmbeddingsEngine::new()),
                ),
                (Route::Exact("stdin"), Box::new(JsonClaimsEngine)),
            ],
        }
    }

    /// Engine identities persisted when a serving world is first created.
    pub(crate) fn names(&self) -> Vec<String> {
        self.routes
            .iter()
            .map(|(_, engine)| engine.name().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Adds `engine` on `route`, after every engine already registered.
    ///
    /// Registering the same name and version on a second route is allowed (overlapping routes
    /// are legitimate configuration) and logged once; [`Self::engines_for`] then returns it
    /// once.
    ///
    /// # Errors
    /// [`RegistryError::VersionConflict`] if an engine of the same name is registered at a
    /// different version.
    pub fn register(
        &mut self,
        route: Route,
        engine: Box<dyn Engine>,
    ) -> Result<&mut Self, RegistryError> {
        let (name, version) = (engine.name(), engine.version());
        if let Some((_, existing)) = self.routes.iter().find(|(_, e)| e.name() == name) {
            if existing.version() != version {
                return Err(RegistryError::VersionConflict {
                    name,
                    registered: existing.version(),
                    rejected: version,
                });
            }
            eprintln!(
                "s2w: bridge: engine '{name}' is registered on more than one route; a source \
                 matching several runs it once"
            );
        }
        self.routes.push((route, engine));
        Ok(self)
    }

    /// Every engine routed for `source`, in registration order, each name once (the first
    /// registration wins). Empty means unrouted.
    #[must_use]
    pub fn engines_for(&self, source: &SourceId) -> Vec<&dyn Engine> {
        let mut engines: Vec<&dyn Engine> = Vec::new();
        for (route, engine) in &self.routes {
            if route.matches(source) && engines.iter().all(|e| e.name() != engine.name()) {
                engines.push(engine.as_ref());
            }
        }
        engines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2w_model::{ModelError, RawEvent};
    use s2w_system1::Verdict;

    fn names(registry: &EngineRegistry, source: &str) -> Result<Vec<&'static str>, ModelError> {
        Ok(registry
            .engines_for(&SourceId::new(source)?)
            .iter()
            .map(|engine| engine.name())
            .collect())
    }

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct Named(&'static str);
    impl Engine for Named {
        fn name(&self) -> &'static str {
            self.0
        }
        fn version(&self) -> u32 {
            1
        }
        fn evaluate(&self, _: &RawEvent) -> Verdict {
            Verdict::Abstain {
                reason: s2w_system1::AbstainReason::NotMine,
            }
        }
    }

    struct Versioned(u32);
    impl Engine for Versioned {
        fn name(&self) -> &'static str {
            "versioned"
        }
        fn version(&self) -> u32 {
            self.0
        }
        fn evaluate(&self, _: &RawEvent) -> Verdict {
            Verdict::Abstain {
                reason: s2w_system1::AbstainReason::NotMine,
            }
        }
    }

    #[test]
    fn defaults_route_wikipedia_and_stdin_and_nothing_else() -> Result<(), ModelError> {
        let registry = EngineRegistry::with_defaults();
        assert_eq!(
            names(&registry, "wikipedia.page_change")?,
            ["wikimedia.page_change", "wikimedia.local_embeddings"]
        );
        assert_eq!(names(&registry, "stdin")?, ["json_claims"]);
        assert!(names(&registry, "kafka.orders")?.is_empty());
        Ok(())
    }

    #[test]
    fn engines_on_one_route_come_back_in_registration_order() -> TestResult {
        let mut registry = EngineRegistry::new();
        registry
            .register(Route::Prefix("a."), Box::new(Named("second-name-first")))?
            .register(Route::Prefix("a."), Box::new(Named("first-name-second")))?;
        assert_eq!(
            names(&registry, "a.b")?,
            ["second-name-first", "first-name-second"]
        );
        Ok(())
    }

    #[test]
    fn a_name_on_overlapping_routes_runs_once() -> TestResult {
        let mut registry = EngineRegistry::new();
        registry
            .register(Route::Prefix("a."), Box::new(Named("one")))?
            .register(Route::Exact("a.b"), Box::new(Named("two")))?
            .register(Route::Exact("a.b"), Box::new(Named("one")))?;
        assert_eq!(names(&registry, "a.b")?, ["one", "two"]);
        assert_eq!(names(&registry, "a.c")?, ["one"]);
        Ok(())
    }

    #[test]
    fn a_name_at_a_second_version_is_refused() -> TestResult {
        let mut registry = EngineRegistry::new();
        registry.register(Route::Prefix("a."), Box::new(Versioned(1)))?;
        let refused = registry
            .register(Route::Exact("b"), Box::new(Versioned(2)))
            .err();
        assert_eq!(
            refused,
            Some(RegistryError::VersionConflict {
                name: "versioned",
                registered: 1,
                rejected: 2
            })
        );
        assert!(names(&registry, "b")?.is_empty(), "nothing was added");
        Ok(())
    }

    #[test]
    fn prefix_includes_its_separator() -> Result<(), ModelError> {
        let registry = EngineRegistry::with_defaults();
        assert!(names(&registry, "wikipediafoo")?.is_empty());
        assert!(names(&registry, "stdin.extra")?.is_empty());
        Ok(())
    }
}
