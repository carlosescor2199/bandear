// Line icons for the instrument kinds and for a stereo pair, drawn on a 24 px grid in the current
// colour. They are decorative: the kind or "stereo" is always written out beside them.

const KIND_PATHS = Object.freeze({
  voice: `<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21M9 21h6"/>`,
  guitar: `<path d="M13.5 10.5 20 4M18 2.5l3.5 3.5"/><path d="M11 9.2a3.6 3.6 0 0 0-4.9 1 3.2 3.2 0 0 1-2.2 1.5 2.8 2.8 0 0 0-1.4 4.7l3.3 3.3a2.8 2.8 0 0 0 4.7-1.4 3.2 3.2 0 0 1 1.5-2.2 3.6 3.6 0 0 0 1-4.9z"/><circle cx="8.6" cy="15.4" r="1.4"/>`,
  bass: `<path d="M12 12 21 3"/><path d="M19 2.5 21.5 5"/><path d="M10 10.5a3.4 3.4 0 0 0-4.6.8 3 3 0 0 1-2 1.3 2.6 2.6 0 0 0-1.2 4.4l2.8 2.8a2.6 2.6 0 0 0 4.4-1.2 3 3 0 0 1 1.3-2 3.4 3.4 0 0 0 .8-4.6"/><path d="M7 14.5l2.5 2.5"/>`,
  keys: `<rect x="2.5" y="5" width="19" height="14" rx="2"/><path d="M7.25 19v-5M12 19v-5M16.75 19v-5"/><path d="M6 5v9h2.5V5M10.75 5v9h2.5V5M15.5 5v9H18V5"/>`,
  drums: `<ellipse cx="12" cy="10" rx="8" ry="3"/><path d="M4 10v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/><path d="M7 3.5l4 5M17 3.5l-4 5"/>`,
  percussion: `<path d="M8 3h8l-1.5 18h-5z"/><path d="M8.4 8h7.2M9.2 16h5.6"/>`,
  horns: `<path d="M2.5 11h8"/><path d="M10.5 7.5 20.5 4v14l-10-3.5z"/><path d="M5 11v3.5M8 11v3.5"/>`,
  strings: `<path d="M9 3c-2.2 0-3.5 1.6-3.5 3.5 0 1.4.8 2.2.8 3.5S5 12.4 5 14.5C5 18.6 8 21 12 21s7-2.4 7-6.5c0-2.1-1.3-3.2-1.3-4.5s.8-2.1.8-3.5C18.5 4.6 17.2 3 15 3c-1.3 0-2 .9-3 .9S10.3 3 9 3z"/><path d="M12 6v15M9.5 12h5"/>`,
  playback: `<circle cx="12" cy="12" r="9"/><path d="M10 8.5v7l6-3.5z"/>`,
  click: `<path d="M8.5 3h7L19 21H5z"/><path d="M12 17l4.5-11"/><path d="M8 17h8"/>`,
  other: `<path d="M2.5 12h3.5l2-5 4 10 3-7 1.5 2h5"/>`,
});

const STEREO_PATH = `<circle cx="8.5" cy="12" r="5.5"/><circle cx="15.5" cy="12" r="5.5"/>`;

function svg(paths, size) {
  return `<svg class="icon" aria-hidden="true" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>`;
}

/** The icon of an instrument kind as SVG markup; an unknown kind draws as "other". */
export function kindIcon(kind, size = 16) {
  return svg(KIND_PATHS[kind] ?? KIND_PATHS.other, size);
}

/** The stereo pair mark as SVG markup: two overlapping rings. */
export function stereoIcon(size = 14) {
  return svg(STEREO_PATH, size);
}
