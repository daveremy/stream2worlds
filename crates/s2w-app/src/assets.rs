//! The committed web bundle, loaded at build time by memory-serve (decision 0016).

pub(crate) fn asset_router() -> axum::Router {
    memory_serve::load!()
        .index_file(Some("/index.html"))
        .cache_control(memory_serve::CacheControl::NoCache)
        .into_router()
}
