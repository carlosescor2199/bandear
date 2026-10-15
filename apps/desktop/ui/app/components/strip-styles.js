// The look the channel, group and master strips share: a tall rounded column of controls whose
// faders take the height that is left.

import { sheet } from "./support.js";

export const stripStyles = sheet(`
  :host {
    display: flex;
    flex: none;
    min-height: 0;
  }
  .strip {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 7px;
    width: 100%;
    min-height: 0;
    padding: 10px 6px 8px;
    border: 1px solid var(--ml-border);
    border-radius: var(--ml-radius-lg);
    background: var(--ml-surface);
  }
  .caption {
    color: var(--ml-text-muted);
    font-size: var(--ml-text-2xs);
    font-weight: var(--ml-weight-bold);
    letter-spacing: var(--ml-tracking-caps);
  }
  .levels {
    display: flex;
    flex: 1 1 auto;
    gap: 6px;
    align-items: stretch;
    min-height: 200px;
  }
  .readout {
    font-family: var(--ml-font-mono);
    font-size: var(--ml-text-sm);
    font-weight: var(--ml-weight-bold);
    white-space: nowrap;
  }
  .buttons {
    display: flex;
    gap: 4px;
  }
`);
