//! Which System 1 engines run on which source. Routing is app composition: engines never know
//! which stream they are attached to, and `s2w_sources::registry` maps URIs to transports, not
//! payloads to engines.

use s2w_model::SourceId;
use s2w_system1::{Engine, JsonClaimsEngine};

/// Which source ids a registered engine runs on. Names are compared as text, so routing a name
/// no source ever produces is not an error; it just never matches.
/// Owned text: routes are built from stored mapping proposals at start-up (decision 0023), not
/// only from literals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// Exactly this source id.
    Exact(String),
    /// Every source id starting with this prefix. Include the separator (`"stdin."`) so
    /// the prefix cannot match a longer sibling name (`"stdinfoo"`).
    Prefix(String),
}

impl Route {
    fn matches(&self, source: &SourceId) -> bool {
        match self {
            Self::Exact(id) => source.as_str() == id,
            Self::Prefix(prefix) => source.as_str().starts_with(prefix.as_str()),
        }
    }

    /// Whether some source id could match both routes.
    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Exact(a), Self::Exact(b)) => a == b,
            (Self::Exact(id), Self::Prefix(prefix)) | (Self::Prefix(prefix), Self::Exact(id)) => {
                id.starts_with(prefix.as_str())
            }
            (Self::Prefix(a), Self::Prefix(b)) => {
                a.starts_with(b.as_str()) || b.starts_with(a.as_str())
            }
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
        name: String,
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

    /// The first-slice routing: `stdin` to the JSON-claims engine. Everything else is
    /// unrouted until a discovery-based engine exists (decision 0018: no compiled code may
    /// key on a stream's domain, so the retired domain-bound engines have no successor here).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self {
            routes: vec![(Route::Exact("stdin".to_owned()), Box::new(JsonClaimsEngine))],
        }
    }

    /// Every registered engine name, sorted and deduplicated: the identities persisted when a
    /// serving world is first created, and the only names whose stored verdicts replay serves
    /// (decision 0023).
    pub(crate) fn names(&self) -> Vec<String> {
        self.routes
            .iter()
            .map(|(_, engine)| engine.name().to_owned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// A fingerprint of what feeds the world: every `(route, engine name)` in registration
    /// order. Engine versions are left out on purpose: a version bump never re-runs history
    /// (stored verdicts keep serving, decision 0012), so a replay after one folds the same
    /// world. Order is kept because the bridge runs matching engines in registration order, so
    /// reordering can reorder claims. A world snapshot records this and is ignored under any
    /// other routing (decision 0024, validity rule 4).
    #[must_use]
    pub fn feed_fingerprint(&self) -> u64 {
        let mut hash = s2w_model::Fnv64::new();
        for (route, engine) in &self.routes {
            let (kind, text) = match route {
                Route::Exact(id) => ("exact", id.as_str()),
                Route::Prefix(prefix) => ("prefix", prefix.as_str()),
            };
            hash.write_field(kind.as_bytes())
                .write_field(text.as_bytes())
                .write_field(engine.name().as_bytes());
        }
        hash.finish()
    }

    /// Adds `engine` on `route`, after every engine already registered.
    ///
    /// Registering the same name and version on a second route is allowed. When the routes
    /// can overlap (legitimate configuration) it is logged; [`Self::engines_for`] then returns
    /// it once. Disjoint routes, such as one mapping accepted for two exact sources, are not
    /// logged.
    ///
    /// # Errors
    /// [`RegistryError::VersionConflict`] if an engine of the same name is registered at a
    /// different version.
    pub fn register(
        &mut self,
        route: Route,
        engine: Box<dyn Engine>,
    ) -> Result<&mut Self, RegistryError> {
        let version = engine.version();
        let name = engine.name();
        if let Some((_, existing)) = self.routes.iter().find(|(_, e)| e.name() == name)
            && existing.version() != version
        {
            return Err(RegistryError::VersionConflict {
                name: name.to_owned(),
                registered: existing.version(),
                rejected: version,
            });
        }
        if self.overlaps_same_name(&route, name) {
            eprintln!(
                "s2w: bridge: engine '{name}' is registered on more than one route; a source \
                 matching several runs it once"
            );
        }
        self.routes.push((route, engine));
        Ok(self)
    }

    /// Whether an engine named `name` is already registered on a route that can overlap `route`.
    fn overlaps_same_name(&self, route: &Route, name: &str) -> bool {
        self.routes
            .iter()
            .any(|(existing, engine)| engine.name() == name && existing.overlaps(route))
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

    fn names(registry: &EngineRegistry, source: &str) -> Result<Vec<String>, ModelError> {
        Ok(registry
            .engines_for(&SourceId::new(source)?)
            .iter()
            .map(|engine| engine.name().to_owned())
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
    fn defaults_route_stdin_and_nothing_else() -> Result<(), ModelError> {
        // Decision 0018's accepted consequence: a preset-sourced event runs no engine from
        // the default registry until a discovery-based engine exists.
        let registry = EngineRegistry::with_defaults();
        assert_eq!(names(&registry, "stdin")?, ["json_claims"]);
        assert!(names(&registry, "wikipedia.page_change")?.is_empty());
        assert!(names(&registry, "kafka.orders")?.is_empty());
        Ok(())
    }

    #[test]
    fn engines_on_one_route_come_back_in_registration_order() -> TestResult {
        let mut registry = EngineRegistry::new();
        registry
            .register(
                Route::Prefix("a.".to_owned()),
                Box::new(Named("second-name-first")),
            )?
            .register(
                Route::Prefix("a.".to_owned()),
                Box::new(Named("first-name-second")),
            )?;
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
            .register(Route::Prefix("a.".to_owned()), Box::new(Named("one")))?
            .register(Route::Exact("a.b".to_owned()), Box::new(Named("two")))?
            .register(Route::Exact("a.b".to_owned()), Box::new(Named("one")))?;
        assert_eq!(names(&registry, "a.b")?, ["one", "two"]);
        assert_eq!(names(&registry, "a.c")?, ["one"]);
        Ok(())
    }

    #[test]
    fn routes_overlap_only_when_one_source_can_match_both() {
        let exact = |id: &str| Route::Exact(id.to_owned());
        let prefix = |p: &str| Route::Prefix(p.to_owned());
        assert!(exact("a.b").overlaps(&exact("a.b")));
        assert!(!exact("one").overlaps(&exact("two")));
        assert!(exact("a.b").overlaps(&prefix("a.")));
        assert!(prefix("a.").overlaps(&exact("a.b")));
        assert!(!prefix("a.").overlaps(&exact("b.a")));
        assert!(prefix("a.").overlaps(&prefix("a.b.")));
        assert!(prefix("a.b.").overlaps(&prefix("a.")));
        assert!(!prefix("a.").overlaps(&prefix("b.")));
    }

    #[test]
    fn a_name_at_a_second_version_is_refused() -> TestResult {
        let mut registry = EngineRegistry::new();
        registry.register(Route::Prefix("a.".to_owned()), Box::new(Versioned(1)))?;
        let refused = registry
            .register(Route::Exact("b".to_owned()), Box::new(Versioned(2)))
            .err();
        assert_eq!(
            refused,
            Some(RegistryError::VersionConflict {
                name: "versioned".to_owned(),
                registered: 1,
                rejected: 2
            })
        );
        assert!(names(&registry, "b")?.is_empty(), "nothing was added");
        Ok(())
    }

    #[test]
    fn prefix_includes_its_separator() -> TestResult {
        let mut registry = EngineRegistry::new();
        registry.register(Route::Prefix("a.".to_owned()), Box::new(Named("one")))?;
        assert_eq!(names(&registry, "a.b")?, ["one"]);
        assert!(
            names(&registry, "afoo")?.is_empty(),
            "no sibling-name match"
        );
        Ok(())
    }

    #[test]
    fn feed_fingerprint_tracks_routes_names_and_order_but_not_versions() -> TestResult {
        let build = |pairs: &[(Route, u32, &'static str)]| -> Result<u64, RegistryError> {
            let mut registry = EngineRegistry::new();
            for (route, version, name) in pairs {
                let engine: Box<dyn Engine> = if *name == "versioned" {
                    Box::new(Versioned(*version))
                } else {
                    Box::new(Named(name))
                };
                registry.register(route.clone(), engine)?;
            }
            Ok(registry.feed_fingerprint())
        };
        let base = build(&[
            (Route::Exact("a".to_owned()), 1, "versioned"),
            (Route::Exact("a".to_owned()), 1, "x"),
        ])?;
        assert_eq!(
            base,
            build(&[
                (Route::Exact("a".to_owned()), 2, "versioned"),
                (Route::Exact("a".to_owned()), 1, "x")
            ])?,
            "a version bump keeps the fingerprint"
        );
        for other in [
            build(&[
                (Route::Exact("a".to_owned()), 1, "x"),
                (Route::Exact("a".to_owned()), 1, "versioned"),
            ])?,
            build(&[
                (Route::Prefix("a".to_owned()), 1, "versioned"),
                (Route::Exact("a".to_owned()), 1, "x"),
            ])?,
            build(&[
                (Route::Exact("b".to_owned()), 1, "versioned"),
                (Route::Exact("a".to_owned()), 1, "x"),
            ])?,
            build(&[(Route::Exact("a".to_owned()), 1, "versioned")])?,
        ] {
            assert_ne!(base, other);
        }
        assert_eq!(
            EngineRegistry::with_defaults().feed_fingerprint(),
            EngineRegistry::with_defaults().feed_fingerprint()
        );
        Ok(())
    }
}
