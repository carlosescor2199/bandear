// Shared helpers for the console's custom elements.
//
// Every atom renders into an open Shadow DOM with constructable style sheets, reads the design
// tokens (custom properties inherit into the shadow tree), takes attributes and properties in and
// sends CustomEvents out. None of them calls Tauri: screens own the state and the bridge.

/** A constructable style sheet, shared by every instance of an element. */
export function sheet(cssText) {
  const styles = new CSSStyleSheet();
  styles.replaceSync(cssText);
  return styles;
}

const base = sheet(`
  :host {
    box-sizing: border-box;
    font-family: var(--ml-font-sans);
    color: var(--ml-text);
  }
  :host([hidden]) {
    display: none !important;
  }
  *,
  *::before,
  *::after {
    box-sizing: inherit;
  }
  @media (prefers-reduced-motion: reduce) {
    *,
    *::before,
    *::after {
      animation: none !important;
      transition: none !important;
    }
  }
`);

/** Attaches the shadow root with the base sheet, the element's sheet and its markup. */
export function attachShadowRoot(element, styles, html, options = {}) {
  const root = element.attachShadow({ mode: "open", ...options });
  root.adoptedStyleSheets = [base, styles];
  root.innerHTML = html;
  return root;
}

/** Registers an element once, so importing a module twice is harmless. */
export function define(name, constructor) {
  if (!customElements.get(name)) {
    customElements.define(name, constructor);
  }
}

/** Dispatches a CustomEvent that bubbles out of the shadow tree. */
export function emit(element, type, detail) {
  element.dispatchEvent(new CustomEvent(type, { detail, bubbles: true, composed: true }));
}

/** A numeric attribute, or `fallback` when it is absent or not a number. */
export function numberAttribute(element, name, fallback) {
  const raw = element.getAttribute(name);
  if (raw === null || raw.trim() === "") {
    return fallback;
  }
  const number = Number(raw);
  return Number.isFinite(number) ? number : fallback;
}

/**
 * Re-applies a property that was set on the element before it was upgraded, so it goes through
 * the class's setter instead of shadowing it.
 */
export function upgradeProperty(element, name) {
  if (Object.prototype.hasOwnProperty.call(element, name)) {
    const value = element[name];
    delete element[name];
    element[name] = value;
  }
}

/** Mirrors the `label` attribute as the host's accessible name. */
export function syncLabel(element) {
  const label = element.getAttribute("label");
  if (label) {
    element.setAttribute("aria-label", label);
  } else {
    element.removeAttribute("aria-label");
  }
}
