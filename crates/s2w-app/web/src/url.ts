// Pure URL-building helpers, kept in their own module (no DOM side effects at import time) so
// they can be unit-tested directly — round-1 review finding: every writer that touches the
// visible URL must go through ONE function, and `world` must never round-trip back into the
// query now that it lives in the path.
const WORLD_PATH = /^\/w\/([^/]+)\//;

/** Extracts the world segment from a `/w/<world>/...` pathname, percent-decoded. */
export function parseWorldFromPath(pathname: string): string | undefined {
  const match = WORLD_PATH.exec(pathname);
  return match ? decodeURIComponent(match[1]) : undefined;
}

/** The canonical path for a world, percent-encoded. */
export function worldPathFor(world: string): string {
  return `/w/${encodeURIComponent(world)}/`;
}

/**
 * Builds an absolute, path-preserving URL string for `history.replaceState`/`location.replace`.
 * `world` is always stripped from the query — it lives in `pathname` instead — so no writer can
 * accidentally reintroduce `?world=` alongside the path form.
 */
export function buildUrl(pathname: string, params: URLSearchParams): string {
  const query = new URLSearchParams(params);
  query.delete('world');
  return `${pathname}?${query.toString()}`;
}
