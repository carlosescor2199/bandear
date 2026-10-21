// The state of one 3D figure, kept in `data-scene` on its [data-3d-host] container so the CSS
// knows what to show. Pure, so node tests can check it.
//
//   (absent)  loading: the static design shows; the canvas is laid out but transparent
//   "live"    a first frame has been drawn: the canvas replaces the static design
//   "static"  the scene failed (load, mount, a frame, a lost GPU context): static design for good
//
// Nothing hides the static design until a scene has really drawn, so a script that throws or a
// scene that never renders leaves the page as it would look without WebGL.

// `state` is the current attribute value (undefined while loading); `event` is "rendered" or
// "failed". Returns the new value, or `state` unchanged for anything else.
export function nextSceneState(state, event) {
  if (state === "static" || event === "failed") return "static";
  if (event === "rendered") return "live";
  return state;
}
