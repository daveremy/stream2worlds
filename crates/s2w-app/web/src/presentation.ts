// No runtime import from `./api` here (deliberately): `ApiError`'s constructor uses a TS
// parameter property, which Node's native strip-only mode cannot parse — fine for the bundled
// build, but it would make this module (imported directly by presentation.test.mjs) unloadable
// under `node --test`. `main.ts` calls `api.presentation()` and hands this module the result.
import type { Palette, WorldPresentation } from './api';
import type { Force2D } from './renderers/force2d';

const root = document.documentElement;
const media = matchMedia('(prefers-color-scheme: light)');
const paletteTokens = ['ground', 'ink', 'accent', 'success', 'warning', 'danger'] as const;
// Neutral chrome (control fill/border, panel borders) has no palette slot of its own, so it is
// derived from the palette instead of hardcoded: a light palette gets light chrome. Set only
// while a palette applies; with none, style.css falls back to its original dark literals.
const derivedTokens: Record<string, string> = {
  '--s2w-line': 'color-mix(in srgb, var(--s2w-ink) 25%, var(--s2w-ground))',
  '--s2w-line-soft': 'color-mix(in srgb, var(--s2w-ink) 14%, var(--s2w-ground))',
  '--s2w-control': 'color-mix(in srgb, var(--s2w-ink) 8%, var(--s2w-ground))',
  '--s2w-control-hover': 'color-mix(in srgb, var(--s2w-accent) 30%, var(--s2w-ground))',
};
const fontTokens: Record<'display' | 'body' | 'mono', string> = {
  display: '--s2w-font-display', body: '--s2w-font-body', mono: '--s2w-font-mono',
};

const titleEl = document.querySelector<HTMLElement>('#world-title');
const taglineEl = document.querySelector<HTMLElement>('#world-tagline');
const descriptionEl = document.querySelector<HTMLElement>('#world-description');
// Captured once, at load, as the fallback for a world with no presentation set.
const defaultTitle = document.title;
const defaultHeading = titleEl?.textContent ?? defaultTitle;
const defaultTagline = taglineEl?.textContent ?? '';

let current: WorldPresentation = {};
let renderer: Force2D | undefined;

// If only one of light/dark exists, use whichever exists rather than falling through to
// hardcoded CSS defaults (round-2 review finding).
function resolvePalette(p: WorldPresentation, light: boolean): Palette | undefined {
  const primary = light ? p.palette_light : p.palette_dark;
  const secondary = light ? p.palette_dark : p.palette_light;
  return primary ?? secondary ?? undefined;
}

function applyColors(): void {
  const palette = resolvePalette(current, media.matches);
  if (palette) {
    for (const token of paletteTokens) root.style.setProperty(`--s2w-${token}`, palette[token]);
    for (const [name, value] of Object.entries(derivedTokens)) root.style.setProperty(name, value);
  } else {
    for (const token of paletteTokens) root.style.removeProperty(`--s2w-${token}`);
    for (const name of Object.keys(derivedTokens)) root.style.removeProperty(name);
  }
  // Native scrollbars/form controls follow only when a light palette is actually applied; reset
  // back to dark otherwise (round-2 review finding — this must go both ways).
  root.style.colorScheme = media.matches && palette ? 'light' : 'dark';
  if (renderer) {
    const style = getComputedStyle(root);
    renderer.setColors({
      ground: style.getPropertyValue('--s2w-ground').trim() || undefined,
      ink: style.getPropertyValue('--s2w-ink').trim() || undefined,
    });
  }
}

/**
 * Applies a world's presentation: palette (as CSS custom properties, re-resolved on every
 * light/dark switch), typefaces, title, tagline, and description. `graphRenderer` is optional so
 * this can run before `Force2D.mount` (colours are then only stored, applied at mount) or after
 * (colours are pushed to the live canvas immediately) — presentation fetches async and either
 * order is possible.
 */
export function applyPresentation(p: WorldPresentation, graphRenderer?: Force2D): void {
  current = p;
  if (graphRenderer) renderer = graphRenderer;
  const typefaces = p.typefaces;
  for (const face of ['display', 'body', 'mono'] as const) {
    const value = typefaces?.[face];
    if (value) root.style.setProperty(fontTokens[face], value);
    else root.style.removeProperty(fontTokens[face]);
  }
  document.title = p.title || defaultTitle;
  if (titleEl) titleEl.textContent = p.title || defaultHeading;
  if (taglineEl) taglineEl.textContent = p.tagline || defaultTagline;
  if (descriptionEl) {
    // Plain-text for this PR (`textContent`, not innerHTML) — no sanitizer exists in this
    // workspace, and adding one is out of scope here. `description` stores markdown; rendering
    // it as markdown is a follow-up.
    if (p.description) { descriptionEl.textContent = p.description; descriptionEl.hidden = false; }
    else { descriptionEl.textContent = ''; descriptionEl.hidden = true; }
  }
  applyColors();
}

media.addEventListener('change', applyColors);
