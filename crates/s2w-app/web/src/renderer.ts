import type { ViewState } from './state';
// Renderer boundary for the future 3D explorer. Only complete snapshots enter this seam.
export interface GraphRenderer {
  mount(element: HTMLElement, state: ViewState): void;
  update(state: ViewState): void;
  destroy(): void;
}
