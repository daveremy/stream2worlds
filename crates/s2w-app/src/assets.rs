//! The committed web bundle, loaded at build time by memory-serve (decision 0016).

/// Bare `/` serves the instance home page (`home.html`, the dashboard of worlds); the world
/// view shell (`index.html`) is reached through `/w/<world>/`, which `serve::world_routing`
/// rewrites to `/index.html` (stream2worlds#144).
pub(crate) fn asset_router() -> axum::Router {
    memory_serve::load!()
        .index_file(Some("/home.html"))
        .cache_control(memory_serve::CacheControl::NoCache)
        .into_router()
        .layer(axum::middleware::map_response(with_csp))
}

/// Second layer behind the per-world stylesheet's write-time denylist (stream2worlds#153): a
/// denylist miss becomes a blocked load, not a network request. Measured against the bundle:
/// one module script per page (`/main.js`, `/home.js`, same origin), a `<style>` element the
/// viewer injects (hence `'unsafe-inline'` for style only), `fetch`/`EventSource` to same
/// origin, a `<canvas>`, and no images, fonts, workers or `eval`. `data:` stays on `img-src`
/// as the one cheap allowance for canvas/inline-image use.
pub(crate) const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'; \
base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

async fn with_csp(mut response: axum::response::Response) -> axum::response::Response {
    response.headers_mut().insert(
        axum::http::header::CONTENT_SECURITY_POLICY,
        axum::http::HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    response
}
