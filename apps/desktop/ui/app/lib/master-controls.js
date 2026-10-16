// The master strip's controls joined to the mix, apart from the DOM so the wiring <ml-master-strip>
// uses can be tested with stand-in faders. No DOM: the parts only need `addEventListener` and a
// `value` (a fader) or `pressed` (a toggle), as <ml-fader> and <ml-toggle> have.
//
// The level and the phone's cap go out as they move (`input`), as every fader of the mix does. The
// engineer's limit goes out when a gesture ends (`change`: the pointer released, a key press, a
// reset), because the desk keeps every limit it is given; while a drag is on, a value shown from
// outside waits (see slider-gesture.js), so a status arriving mid-drag never moves the thumb.

/**
 * Calls `edit(field, value)` for every change the engineer makes on `parts` (`{ level, cap, limit,
 * mute }`): "volumePercent" and "maxLevelPercent" as they move, "engineerLimitPercent" when a
 * gesture on the limit ends, "muted" when Mute is pressed.
 */
export function wireMasterControls({ level, cap, limit, mute }, edit) {
  level.addEventListener("input", (event) => edit("volumePercent", event.detail.value));
  cap.addEventListener("input", (event) => edit("maxLevelPercent", event.detail.value));
  limit.addEventListener("change", (event) => edit("engineerLimitPercent", event.detail.value));
  mute.addEventListener("change", (event) => edit("muted", event.detail.pressed));
}

/** Shows the master `data` (from masterStrip, mix-view.js) on `parts`. */
export function showMasterValues({ level, cap, limit, mute }, data) {
  level.value = data.volume;
  cap.value = data.cap;
  limit.value = data.limit;
  mute.pressed = data.muted;
}
