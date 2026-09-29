import { presentation, worlds, type WorldPresentation } from './api';
import { buildCard, renderWorlds } from './home';

async function start(signal: AbortSignal, container: HTMLElement, status: HTMLElement): Promise<void> {
  const list = await worlds(signal);
  const cards = await Promise.all(list.worlds.map(async summary => {
    const pres = await presentation(summary.world, signal).catch(error => {
      console.warn(`presentation for ${summary.world} unavailable`, error);
      return {} as WorldPresentation;
    });
    return buildCard(summary, pres);
  }));
  renderWorlds(container, cards);
  status.textContent = '';
}

const status = document.querySelector<HTMLElement>('#home-status')!;
const container = document.querySelector<HTMLElement>('#worlds')!;
const controller = new AbortController();
window.addEventListener('pagehide', () => controller.abort());
start(controller.signal, container, status).catch(error => {
  if (controller.signal.aborted) return;
  status.textContent = `Could not load worlds: ${error instanceof Error ? error.message : error}`;
});
