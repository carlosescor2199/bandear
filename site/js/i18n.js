// English and Spanish copy for the landing page, and the language toggle.
//
// Markup names its strings with attributes; the dictionary below holds every one of them:
//   data-i18n="key"                    sets the element's text
//   data-i18n-html="key"               sets the element's markup (static, trusted strings only)
//   data-i18n-attr="alt=key;content=key" sets attributes
// Strings built by scripts (the download button) use t(key, vars) and listen for the
// "bandear:language" event on document to re-render.
//
// This module touches nothing on import, so node tests can load the dictionary.

import { LICENSE_PAGE, RELEASES_PAGE } from "./links.js";

export const LANGUAGES = Object.freeze(["en", "es"]);
export const DEFAULT_LANGUAGE = "en";
// 404.html repeats STORAGE_KEY and pickLanguage inline (it loads no modules); change both
// together. tests/i18n.test.mjs fails when they drift apart.
export const STORAGE_KEY = "bandear.lang";
export const LANGUAGE_EVENT = "bandear:language";

export const DICTIONARY = Object.freeze({
  en: {
    "meta.title": "BandEar — personal in-ear mixes on every musician's phone",
    "meta.description":
      "BandEar is a free, open-source personal monitor system. A computer and your mixer's USB audio (or any audio interface) run the desk; every musician mixes their own in-ear sound on their own phone over Wi-Fi.",
    "meta.ogImageAlt": "The BandEar mark and name: personal in-ear mixes on every musician's phone",

    "skip": "Skip to content",
    "brand.home": "BandEar home",
    "nav.label": "Main",
    "nav.how": "How it works",
    "nav.features": "Features",
    "nav.download": "Download",
    "nav.faq": "FAQ",
    "lang.toggle": "ES",
    "lang.toggleLabel": "Cambiar a español",
    "lang.toggleLang": "es",
    "github.label": "BandEar on GitHub",

    "hero.eyebrow": "Free · Open source · Personal monitoring",
    "hero.title": 'Every musician gets their own in&#8209;ear mix, <span class="accent">on their own phone.</span>',
    "hero.lede":
      "Plug your mixer's USB audio — or any audio interface — into a computer running BandEar. Each musician joins the same Wi-Fi, opens the app and mixes their own ears — while the engineer keeps control from the desk.",
    "hero.other": "Other platforms",
    "hero.platforms": "Windows · macOS · Linux · Android 10+",
    "hero.shotAlt": "The BandEar console: one tab per musician, channel strips, groups and the master with its limiter",

    "dl.hero.generic": "Download",
    "dl.hero.for": "Download for {os}",
    "dl.hero.ios": "iOS coming soon",
    "dl.os.windows": "Windows",
    "dl.os.mac": "macOS",
    "dl.os.macArm": "Mac (Apple Silicon)",
    "dl.os.linux": "Linux",
    "dl.os.android": "Android",
    "dl.alt.intel": "Using an Intel Mac?",
    "dl.alt.intelLink": "Download the Intel version",
    "dl.alt.deb": "Prefer a .deb package?",
    "dl.alt.debLink": "Download the .deb",
    "dl.alt.linuxArm": "Linux builds are x86-64 only for now; there is no ARM version yet.",
    "dl.alt.ios": "Musicians on iPhone can use any Android phone or tablet for now.",

    "stats.label": "At a glance",
    "stats.latency": "added processing latency",
    "stats.inputs": "inputs",
    "stats.musicians": "musicians at ~18% engine load",
    "stats.cost": "no extra hardware, MIT licensed",

    "how.eyebrow": "How it works",
    "how.title": "From your mixer to every pair of ears in three steps",
    "diagram.title":
      "A mixer's built-in USB audio interface (or any audio interface) feeds a computer running BandEar, which sends one mix per musician to their phones over Wi-Fi.",
    "diagram.mixer": "Mixer · USB audio",
    "diagram.desktop": "BandEar desktop",
    "diagram.singer": "Singer",
    "diagram.drummer": "Drummer",
    "diagram.keys": "Keys",
    "step1.title": "Connect your mixer",
    "step1.body":
      "Most digital mixers have a USB audio interface built in — BandEar uses it. A standalone audio interface works too. Plug it into a Windows, macOS or Linux computer, open BandEar, pick it and press <em>Start engine</em>.",
    "step2.title": "Run the desk",
    "step2.body":
      "The desktop app builds one mix per musician with channel processing, limits and presets. No mixer at hand? The demo source plays eight generated channels.",
    "step3.title": "Connect the phones",
    "step3.body":
      "Musicians join the same Wi-Fi and open the BandEar app. Their phones find the desk on their own — no IP addresses to type — and everyone mixes their own ears.",

    "features.eyebrow": "Features",
    "features.title": "A real monitor desk, without the extra hardware",
    "f1.title": "Personal mixes",
    "f1.i1": "One mix per musician, from their phone or the desk",
    "f1.i2": "Up to 32 inputs, stereo pairs and groups",
    "f1.i3": "Engineer limits and locks, presets, copy and mirror mixes",
    "f1.i4": "Saved sessions and automatic mix backups",
    "f2.title": "Processing",
    "f2.i1": "High-pass, 4-band EQ, gate and compressor on every input",
    "f2.i2": "A limiter on every musician's master",
    "f2.i3": "Shared reverb and delay with sends and per-mix returns",
    "f2.i4": "0 ms of added latency",
    "f3.title": "Recording",
    "f3.i1": "Multitrack: every input on its own WAV track",
    "f3.i2": "Markers, disk-space estimate and crash-safe files",
    "f3.i3": "Virtual soundcheck: play a take back as if the band were playing",
    "f3.i4": "A local mix for the computer's own outputs",
    "f4.title": "Network you can trust",
    "f4.i1": "Link health per musician: loss, glitches and round trip",
    "f4.i2": "Set each phone's buffer from the desk",
    "f4.i3": "Redundancy: every packet sent twice when Wi-Fi gets rough",
    "f4.i4": "Engine load meter, log file and a demo source",

    "shots.eyebrow": "A closer look",
    "shots.title": "Built for the desk",
    "shots.mixAlt": "Mix view: one tab per musician, channel strips, groups and the master with its limiter",
    "shots.mix": "Personal mixes",
    "shots.processingAlt": "The processing editor with the equaliser curve, gate and compressor",
    "shots.processing": "Channel processing",
    "shots.recordAlt": "The Record screen with one track per input and the virtual soundcheck",
    "shots.record": "Recording and virtual soundcheck",
    "shots.networkAlt": "Network and system: link health, buffer and redundancy for every phone",
    "shots.network": "Network and system",

    "downloads.eyebrow": "Download",
    "downloads.title": "Get BandEar for every device in the band",
    "downloads.intro": `The desk runs on a computer; musicians use the Android app. All files come from the <a href="${RELEASES_PAGE}">latest GitHub release</a>.`,
    "dl.windows.note": "First run: <em>More info → Run anyway</em> — the installer is not signed yet.",
    "dl.windows.btn": "Download .exe",
    "dl.mac.note": "Drag to Applications. First run: right-click → <em>Open</em>.",
    "dl.mac.btn": "Download .dmg",
    "dl.linux.note": "<code>chmod +x</code> the AppImage and run it, or install the .deb.",
    "dl.android.muted": "10+ · musicians' app",
    "dl.android.note": "Allow installs from your browser, then open the APK.",
    "dl.android.btn": "Download .apk",
    "dl.ios.file": "Coming soon",
    "dl.ios.note": "An iOS app for musicians is on the roadmap.",
    "dl.ios.btn": "See the roadmap",

    "faq.eyebrow": "FAQ",
    "faq.title": "Questions musicians ask",
    "faq.q1": "Is it free?",
    "faq.a1": "Yes. BandEar is free and open source under the MIT license — no subscriptions, no accounts, no ads.",
    "faq.q2": "Do I need special hardware?",
    "faq.a2":
      "No. You need a computer, a mixer with USB audio (or any audio interface the computer sees as an input) and a Wi-Fi network. Musicians use their own phones and in-ear headphones. A dedicated 5 GHz access point close to the stage works best.",
    "faq.q3": "Which phones work?",
    "faq.a3": "Android 10 or newer. Each phone finds the desk on the same Wi-Fi by itself.",
    "faq.q4": "What about latency?",
    "faq.a4":
      "Processing adds 0 ms: filters and dynamics have no look-ahead and effects are wet returns. The engine builds every mix once per 5.3 ms block, and with 32 inputs and 16 musicians it uses about 18% of each block. The total delay you hear depends on your Wi-Fi and each phone's buffer, which you can set from the desk.",
    "faq.q5": "Is there an iOS app?",
    "faq.a5": "Not yet — an iOS app for musicians is on the roadmap.",
    "faq.q6": "Why does my computer show a security warning?",
    "faq.a6":
      "The installers are not signed yet. On Windows choose <em>More info → Run anyway</em>; on macOS right-click the app and choose <em>Open</em> the first time. Signed installers are on the roadmap.",

    "sponsor.title": "Free, open source, built in spare time",
    "sponsor.body":
      "If BandEar helps your band, church, studio or school, sponsoring helps keep it going. Starring the repo and sharing it with other musicians helps too.",
    "sponsor.btn": "Sponsor on GitHub",
    "sponsor.source": "View the source",

    "footer.license": `© 2026 Carlos Escudero · Released under the <a href="${LICENSE_PAGE}">MIT License</a>`,
    "footer.label": "Footer",
    "footer.releases": "Releases",
    "footer.issues": "Issues",
    "footer.sponsor": "Sponsor",
  },

  es: {
    "meta.title": "BandEar — mezclas in-ear personales en el teléfono de cada músico",
    "meta.description":
      "BandEar es un sistema de monitoreo personal gratuito y de código abierto. Una computadora y el audio USB de tu consola (o cualquier interfaz de audio) manejan la mesa; cada músico ajusta su propia mezcla in-ear desde su teléfono por Wi-Fi.",
    "meta.ogImageAlt": "El logotipo y el nombre de BandEar: mezclas in-ear personales en el teléfono de cada músico",

    "skip": "Saltar al contenido",
    "brand.home": "Inicio de BandEar",
    "nav.label": "Principal",
    "nav.how": "Cómo funciona",
    "nav.features": "Funciones",
    "nav.download": "Descargar",
    "nav.faq": "Preguntas",
    "lang.toggle": "EN",
    "lang.toggleLabel": "Switch to English",
    "lang.toggleLang": "en",
    "github.label": "BandEar en GitHub",

    "hero.eyebrow": "Gratis · Código abierto · Monitoreo personal",
    "hero.title": 'Cada músico tiene su propia mezcla in&#8209;ear, <span class="accent">en su propio teléfono.</span>',
    "hero.lede":
      "Conecta el audio USB de tu consola —o cualquier interfaz de audio— a una computadora con BandEar. Cada músico se une a la misma red Wi-Fi, abre la app y ajusta su propia mezcla, mientras el ingeniero mantiene el control desde la mesa.",
    "hero.other": "Otras plataformas",
    "hero.platforms": "Windows · macOS · Linux · Android 10+",
    "hero.shotAlt": "La consola de BandEar: una pestaña por músico, canales, grupos y el máster con su limitador",

    "dl.hero.generic": "Descargar",
    "dl.hero.for": "Descargar para {os}",
    "dl.hero.ios": "iOS muy pronto",
    "dl.os.windows": "Windows",
    "dl.os.mac": "macOS",
    "dl.os.macArm": "Mac (Apple Silicon)",
    "dl.os.linux": "Linux",
    "dl.os.android": "Android",
    "dl.alt.intel": "¿Tienes una Mac con Intel?",
    "dl.alt.intelLink": "Descarga la versión para Intel",
    "dl.alt.deb": "¿Prefieres un paquete .deb?",
    "dl.alt.debLink": "Descarga el .deb",
    "dl.alt.linuxArm": "Por ahora las versiones para Linux son solo para x86-64; todavía no hay versión para ARM.",
    "dl.alt.ios": "Por ahora, los músicos con iPhone pueden usar cualquier teléfono o tableta Android.",

    "stats.label": "En resumen",
    "stats.latency": "de latencia añadida por el procesamiento",
    "stats.inputs": "entradas",
    "stats.musicians": "músicos con ~18% de carga del motor",
    "stats.cost": "sin hardware extra, licencia MIT",

    "how.eyebrow": "Cómo funciona",
    "how.title": "De tu consola a cada par de oídos en tres pasos",
    "diagram.title":
      "La interfaz de audio USB integrada de una consola (o cualquier interfaz de audio) alimenta una computadora con BandEar, que envía una mezcla por músico a sus teléfonos por Wi-Fi.",
    "diagram.mixer": "Consola · audio USB",
    "diagram.desktop": "BandEar escritorio",
    "diagram.singer": "Cantante",
    "diagram.drummer": "Baterista",
    "diagram.keys": "Teclados",
    "step1.title": "Conecta tu consola",
    "step1.body":
      "La mayoría de las consolas digitales traen una interfaz de audio USB integrada, y BandEar la usa. También sirve una interfaz de audio independiente. Conéctala a una computadora con Windows, macOS o Linux, abre BandEar, selecciónala y pulsa <em>Start engine</em>.",
    "step2.title": "Maneja la mesa",
    "step2.body":
      "La app de escritorio arma una mezcla por músico con procesamiento por canal, límites y presets. ¿No tienes una consola a mano? La fuente de demostración reproduce ocho canales generados.",
    "step3.title": "Conecta los teléfonos",
    "step3.body":
      "Los músicos se unen a la misma red Wi-Fi y abren la app de BandEar. Sus teléfonos encuentran la mesa solos —sin escribir direcciones IP— y cada uno ajusta su propia mezcla.",

    "features.eyebrow": "Funciones",
    "features.title": "Una mesa de monitoreo de verdad, sin hardware extra",
    "f1.title": "Mezclas personales",
    "f1.i1": "Una mezcla por músico, desde su teléfono o desde la mesa",
    "f1.i2": "Hasta 32 entradas, pares estéreo y grupos",
    "f1.i3": "Límites y bloqueos del ingeniero, presets, copia y espejo de mezclas",
    "f1.i4": "Sesiones guardadas y respaldos automáticos de las mezclas",
    "f2.title": "Procesamiento",
    "f2.i1": "Pasa altos, EQ de 4 bandas, gate y compresor en cada entrada",
    "f2.i2": "Un limitador en el máster de cada músico",
    "f2.i3": "Reverb y delay compartidos con envíos y retornos por mezcla",
    "f2.i4": "0 ms de latencia añadida",
    "f3.title": "Grabación",
    "f3.i1": "Multipista: cada entrada en su propia pista WAV",
    "f3.i2": "Marcadores, estimación de espacio en disco y archivos a prueba de fallos",
    "f3.i3": "Prueba de sonido virtual: reproduce una toma como si la banda estuviera tocando",
    "f3.i4": "Una mezcla local para las salidas de la propia computadora",
    "f4.title": "Una red confiable",
    "f4.i1": "Estado del enlace por músico: pérdidas, cortes y tiempo de ida y vuelta",
    "f4.i2": "Ajusta el búfer de cada teléfono desde la mesa",
    "f4.i3": "Redundancia: cada paquete se envía dos veces cuando el Wi-Fi falla",
    "f4.i4": "Medidor de carga del motor, archivo de registro y fuente de demostración",

    "shots.eyebrow": "Más de cerca",
    "shots.title": "Hecho para la mesa",
    "shots.mixAlt": "Vista de mezcla: una pestaña por músico, canales, grupos y el máster con su limitador",
    "shots.mix": "Mezclas personales",
    "shots.processingAlt": "El editor de procesamiento con la curva del ecualizador, el gate y el compresor",
    "shots.processing": "Procesamiento por canal",
    "shots.recordAlt": "La pantalla de grabación con una pista por entrada y la prueba de sonido virtual",
    "shots.record": "Grabación y prueba de sonido virtual",
    "shots.networkAlt": "Red y sistema: estado del enlace, búfer y redundancia de cada teléfono",
    "shots.network": "Red y sistema",

    "downloads.eyebrow": "Descarga",
    "downloads.title": "Descarga BandEar para cada dispositivo de la banda",
    "downloads.intro": `La mesa funciona en una computadora; los músicos usan la app de Android. Todos los archivos vienen de la <a href="${RELEASES_PAGE}">última versión en GitHub</a>.`,
    "dl.windows.note": "Primera vez: <em>Más información → Ejecutar de todas formas</em>; el instalador aún no está firmado.",
    "dl.windows.btn": "Descargar .exe",
    "dl.mac.note": "Arrástrala a Aplicaciones. Primera vez: clic derecho → <em>Abrir</em>.",
    "dl.mac.btn": "Descargar .dmg",
    "dl.linux.note": "Dale permiso de ejecución al AppImage con <code>chmod +x</code> y ábrelo, o instala el .deb.",
    "dl.android.muted": "10+ · app para músicos",
    "dl.android.note": "Permite las instalaciones desde tu navegador y luego abre el APK.",
    "dl.android.btn": "Descargar .apk",
    "dl.ios.file": "Muy pronto",
    "dl.ios.note": "Una app de iOS para músicos está en la hoja de ruta.",
    "dl.ios.btn": "Ver la hoja de ruta",

    "faq.eyebrow": "Preguntas frecuentes",
    "faq.title": "Lo que preguntan los músicos",
    "faq.q1": "¿Es gratis?",
    "faq.a1": "Sí. BandEar es gratuito y de código abierto bajo la licencia MIT: sin suscripciones, sin cuentas y sin anuncios.",
    "faq.q2": "¿Necesito hardware especial?",
    "faq.a2":
      "No. Necesitas una computadora, una consola con audio USB (o cualquier interfaz de audio que la computadora reconozca como entrada) y una red Wi-Fi. Los músicos usan sus propios teléfonos y audífonos in-ear. Lo ideal es un punto de acceso de 5 GHz dedicado y cerca del escenario.",
    "faq.q3": "¿Qué teléfonos funcionan?",
    "faq.a3": "Android 10 o posterior. Cada teléfono encuentra la mesa por sí solo en la misma red Wi-Fi.",
    "faq.q4": "¿Y la latencia?",
    "faq.a4":
      "El procesamiento añade 0 ms: los filtros y la dinámica no usan look-ahead y los efectos son retornos en paralelo. El motor arma cada mezcla una vez por bloque de 5.3 ms y, con 32 entradas y 16 músicos, usa cerca del 18% de cada bloque. El retardo total que escuchas depende de tu Wi-Fi y del búfer de cada teléfono, que puedes ajustar desde la mesa.",
    "faq.q5": "¿Hay una app para iOS?",
    "faq.a5": "Todavía no; una app de iOS para músicos está en la hoja de ruta.",
    "faq.q6": "¿Por qué mi computadora muestra una advertencia de seguridad?",
    "faq.a6":
      "Los instaladores aún no están firmados. En Windows elige <em>Más información → Ejecutar de todas formas</em>; en macOS haz clic derecho en la app y elige <em>Abrir</em> la primera vez. Los instaladores firmados están en la hoja de ruta.",

    "sponsor.title": "Gratis, de código abierto y hecho en el tiempo libre",
    "sponsor.body":
      "Si BandEar le sirve a tu banda, iglesia, estudio o escuela, patrocinarlo ayuda a mantenerlo vivo. Darle una estrella al repositorio y compartirlo con otros músicos también ayuda.",
    "sponsor.btn": "Patrocinar en GitHub",
    "sponsor.source": "Ver el código fuente",

    "footer.license": `© 2026 Carlos Escudero · Publicado bajo la <a href="${LICENSE_PAGE}">licencia MIT</a>`,
    "footer.label": "Pie de página",
    "footer.releases": "Versiones",
    "footer.issues": "Problemas",
    "footer.sponsor": "Patrocinar",
  },
});

let current = DEFAULT_LANGUAGE;

export function currentLanguage() {
  return current;
}

// The string for `key` in `lang`, with {name} placeholders filled from `vars`.
// Missing keys fall back to English, then to the key itself, so a gap never blanks the page.
export function translate(lang, key, vars = {}) {
  const table = DICTIONARY[lang] ?? DICTIONARY[DEFAULT_LANGUAGE];
  const text = table[key] ?? DICTIONARY[DEFAULT_LANGUAGE][key] ?? key;
  return text.replace(/\{(\w+)\}/g, (match, name) => (name in vars ? String(vars[name]) : match));
}

export function t(key, vars) {
  return translate(current, key, vars);
}

// A stored choice wins; otherwise any Spanish browser language picks Spanish.
export function pickLanguage(stored, browserLanguages = []) {
  if (LANGUAGES.includes(stored)) return stored;
  const first = browserLanguages.find((tag) => typeof tag === "string" && tag.length > 0);
  return first && first.toLowerCase().startsWith("es") ? "es" : DEFAULT_LANGUAGE;
}

// Parses "alt=key;aria-label=other" into [["alt", "key"], ["aria-label", "other"]].
export function parseAttrSpec(spec) {
  return spec
    .split(";")
    .map((pair) => pair.trim())
    .filter(Boolean)
    .map((pair) => {
      const at = pair.indexOf("=");
      return [pair.slice(0, at).trim(), pair.slice(at + 1).trim()];
    });
}

// Every dictionary key a page's markup refers to (used by the tests).
export function keysInHtml(html) {
  const keys = new Set();
  for (const [, key] of html.matchAll(/data-i18n(?:-html)?="([^"]+)"/g)) keys.add(key);
  for (const [, spec] of html.matchAll(/data-i18n-attr="([^"]+)"/g)) {
    for (const [, key] of parseAttrSpec(spec)) keys.add(key);
  }
  return keys;
}

// Rewrites every tagged element under `root` in `lang`.
export function applyLanguage(lang, root = document) {
  current = LANGUAGES.includes(lang) ? lang : DEFAULT_LANGUAGE;
  document.documentElement.lang = current;
  for (const element of root.querySelectorAll("[data-i18n]")) {
    element.textContent = t(element.dataset.i18n);
  }
  for (const element of root.querySelectorAll("[data-i18n-html]")) {
    element.innerHTML = t(element.dataset.i18nHtml);
  }
  for (const element of root.querySelectorAll("[data-i18n-attr]")) {
    for (const [name, key] of parseAttrSpec(element.dataset.i18nAttr)) {
      element.setAttribute(name, t(key));
    }
  }
  document.dispatchEvent(new CustomEvent(LANGUAGE_EVENT, { detail: { lang: current } }));
}

function readStored() {
  try {
    return window.localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function store(lang) {
  try {
    window.localStorage.setItem(STORAGE_KEY, lang);
  } catch {
    // Private mode or blocked storage: the choice lasts for this visit only.
  }
}

// Applies the remembered or browser language and wires the toggle button.
export function initI18n() {
  const languages = navigator.languages?.length ? navigator.languages : [navigator.language];
  const lang = pickLanguage(readStored(), languages);
  // The markup ships in English; only rewrite it when another language is wanted.
  if (lang !== DEFAULT_LANGUAGE) applyLanguage(lang);
  else document.documentElement.lang = DEFAULT_LANGUAGE;

  const toggle = document.querySelector(".lang-toggle");
  toggle?.addEventListener("click", () => {
    const next = current === "es" ? "en" : "es";
    store(next);
    applyLanguage(next);
  });
}
