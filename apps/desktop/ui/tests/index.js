// `node --test apps/desktop/ui/tests` loads this folder as one module, so this file imports every
// suite. `node --test` run inside apps/desktop/ui finds the *.test.js files on its own instead.
import "./store.test.js";
import "./values.test.js";
import "./format.test.js";
import "./mix.test.js";
import "./console-state.test.js";
import "./coalesce.test.js";
import "./bridge.test.js";
import "./router.test.js";
import "./slider-gesture.test.js";
import "./engine-config.test.js";
import "./mix-view.test.js";
import "./console-app.test.js";
