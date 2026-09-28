//! The committed web bundle, loaded at build time by memory-serve (decision 0016).

/// Bare `/` serves the instance home page (`home.html`, the dashboard of worlds); the world
/// view shell (`index.html`) is reached through `/w/<world>/`, which `serve::world_routing`
/// rewrites to `/index.html` (stream2worlds#144).
pub(crate) fn asset_router() -> axum::Router {
    memory_serve::load!()
        .index_file(Some("/home.html"))
        .cache_control(memory_serve::CacheControl::NoCache)
        .into_router()
}
