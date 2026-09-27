//! Which System 1 engines run on which source. Routing is app composition: engines never know
//! which stream they are attached to, and `s2w_sources::registry` maps URIs to transports, not
//! payloads to engines.

use s2w_model::SourceId;
use s2w_system1::{Engine, JsonClaimsEngine, WikimediaPageChangeEngine};

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

/// Engines keyed by route, in registration order.
///
/// Several engines may match one source; the bridge runs all of them in registration order,
/// so the resulting timeline is a deterministic function of the log and this registry.
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

    /// The first-slice routing: Wikimedia page changes to the rules engine, `stdin` to the
    /// JSON-claims engine.
    #[must_use]
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(
            Route::Prefix("wikipedia."),
            Box::new(WikimediaPageChangeEngine),
        );
        registry.register(Route::Exact("stdin"), Box::new(JsonClaimsEngine));
        registry
    }

    /// Adds `engine` on `route`, after every engine already registered.
    pub fn register(&mut self, route: Route, engine: Box<dyn Engine>) -> &mut Self {
        self.routes.push((route, engine));
        self
    }

    /// Every engine routed for `source`, in registration order. Empty means unrouted.
    #[must_use]
    pub fn engines_for(&self, source: &SourceId) -> Vec<&dyn Engine> {
        self.routes
            .iter()
            .filter(|(route, _)| route.matches(source))
            .map(|(_, engine)| engine.as_ref())
            .collect()
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

    #[test]
    fn defaults_route_wikipedia_and_stdin_and_nothing_else() -> Result<(), ModelError> {
        let registry = EngineRegistry::with_defaults();
        assert_eq!(
            names(&registry, "wikipedia.page_change")?,
            ["wikimedia.page_change"]
        );
        assert_eq!(names(&registry, "stdin")?, ["json_claims"]);
        assert!(names(&registry, "kafka.orders")?.is_empty());
        Ok(())
    }

    #[test]
    fn engines_on_one_route_come_back_in_registration_order() -> Result<(), ModelError> {
        let mut registry = EngineRegistry::new();
        registry
            .register(Route::Prefix("a."), Box::new(Named("second-name-first")))
            .register(Route::Prefix("a."), Box::new(Named("first-name-second")));
        assert_eq!(
            names(&registry, "a.b")?,
            ["second-name-first", "first-name-second"]
        );
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
