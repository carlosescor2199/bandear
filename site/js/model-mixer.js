// A generic compact mixing console, low-poly: a sloped desk with channel strips (knobs, fader,
// LED meter), a master section (two faders, a meter pair, a small screen) and a USB port on the
// rear panel — the console's built-in audio interface. Deliberately unbranded.

import * as THREE from "../vendor/three/three.module.min.js";

const WIDTH = 2.6;
const DEPTH = 1.5;
const FRONT_HEIGHT = 0.12;
const BACK_HEIGHT = 0.42;
const STRIPS = 8;
const STRIP_LEDS = 6;
const MASTER_LEDS = 10;

const LED_ON = new THREE.Color(0xffbf43);
const LED_OFF = new THREE.Color(0x3a2e14);

// `materials` needs body, screen, edge and lit (see scene-flow.js).
export function buildMixer(materials) {
  const group = new THREE.Group();
  const owned = []; // geometries/materials created here, freed by dispose()
  const own = (thing) => (owned.push(thing), thing);

  // Body: a wedge (side profile extruded across the width), lower at the front.
  const profile = new THREE.Shape([
    new THREE.Vector2(-DEPTH / 2, 0),
    new THREE.Vector2(DEPTH / 2, 0),
    new THREE.Vector2(DEPTH / 2, BACK_HEIGHT),
    new THREE.Vector2(-DEPTH / 2, FRONT_HEIGHT),
  ]);
  const bodyGeometry = own(new THREE.ExtrudeGeometry(profile, { depth: WIDTH, bevelEnabled: false }));
  // Profile x → world -z (front at +z), extrusion → world x, centred.
  bodyGeometry.rotateY(Math.PI / 2).translate(-WIDTH / 2, 0, 0);
  group.add(new THREE.Mesh(bodyGeometry, materials.body));
  group.add(new THREE.LineSegments(own(new THREE.EdgesGeometry(bodyGeometry, 20)), materials.edge));

  // Everything on the desk lives in `panel`, whose local XZ plane is the sloped top surface.
  const panel = new THREE.Group();
  panel.position.set(0, (FRONT_HEIGHT + BACK_HEIGHT) / 2, 0);
  panel.rotation.x = Math.atan((BACK_HEIGHT - FRONT_HEIGHT) / DEPTH);
  group.add(panel);

  const capMaterial = own(new THREE.MeshStandardMaterial({ color: 0xc9d1cf, roughness: 0.5 }));
  const ledMaterial = own(new THREE.MeshBasicMaterial({ color: 0xffffff }));
  const instanced = (geometry, material, count) => {
    const mesh = new THREE.InstancedMesh(own(geometry), material, count);
    panel.add(mesh);
    return mesh;
  };

  const stripX = (i) => -1.12 + i * 0.2;
  const masterFaders = [0.74, 0.88];
  const masterMeters = [1.03, 1.1];
  const faderXs = [...Array.from({ length: STRIPS }, (_, i) => stripX(i)), ...masterFaders];

  const knobs = instanced(new THREE.CylinderGeometry(0.045, 0.052, 0.05, 12).translate(0, 0.025, 0), materials.body, STRIPS * 3);
  const pointers = instanced(new THREE.BoxGeometry(0.012, 0.012, 0.04).translate(0, 0, 0.02), materials.lit, STRIPS * 3);
  const slots = instanced(new THREE.BoxGeometry(0.022, 0.006, 0.6), materials.screen, faderXs.length);
  const caps = instanced(new THREE.BoxGeometry(0.09, 0.05, 0.055).translate(0, 0.025, 0), capMaterial, faderXs.length);
  const ledCount = STRIPS * STRIP_LEDS + masterMeters.length * MASTER_LEDS;
  const leds = instanced(new THREE.BoxGeometry(0.036, 0.012, 0.036), ledMaterial, ledCount);

  const matrix = new THREE.Matrix4();
  const at = (x, y, z, angle = 0) =>
    matrix.compose(new THREE.Vector3(x, y, z), new THREE.Quaternion().setFromAxisAngle(THREE.Object3D.DEFAULT_UP, angle), new THREE.Vector3(1, 1, 1));

  // Channel strips: three knobs at the back, a fader slot at the front, an LED column beside it.
  for (let i = 0; i < STRIPS; i += 1) {
    for (let k = 0; k < 3; k += 1) {
      const z = -0.62 + k * 0.16;
      const angle = Math.sin(i * 2.1 + k * 1.3) * 2.2;
      knobs.setMatrixAt(i * 3 + k, at(stripX(i), 0, z));
      pointers.setMatrixAt(i * 3 + k, at(stripX(i), 0.051, z, angle));
    }
  }
  faderXs.forEach((x, i) => slots.setMatrixAt(i, at(x, 0.003, 0.3)));

  // LED positions: each column fills from the front (bottom of the meter) toward the back.
  const ledColumns = [];
  let led = 0;
  const addColumn = (x, count, zFront, zBack) => {
    const first = led;
    for (let j = 0; j < count; j += 1) {
      leds.setMatrixAt(led, at(x, 0.006, zFront + ((zBack - zFront) * j) / (count - 1)));
      leds.setColorAt(led, LED_OFF);
      led += 1;
    }
    ledColumns.push({ first, count });
  };
  for (let i = 0; i < STRIPS; i += 1) addColumn(stripX(i) + 0.066, STRIP_LEDS, 0.56, 0.12);
  for (const x of masterMeters) addColumn(x, MASTER_LEDS, 0.58, 0.02);

  // Master screen, glowing faintly, with a few level lines.
  const screen = new THREE.Mesh(own(new THREE.PlaneGeometry(0.5, 0.26).rotateX(-Math.PI / 2)), own(new THREE.MeshBasicMaterial({ color: 0x241e0f })));
  screen.position.set(0.92, 0.004, -0.5);
  panel.add(screen);
  const screenEdge = new THREE.LineSegments(own(new THREE.EdgesGeometry(screen.geometry)), materials.edge);
  screenEdge.position.copy(screen.position);
  panel.add(screenEdge);
  for (let j = 0; j < 3; j += 1) {
    const line = new THREE.Mesh(own(new THREE.BoxGeometry(0.3 - j * 0.07, 0.002, 0.018)), materials.lit);
    line.position.set(0.86 - j * 0.035, 0.007, -0.56 + j * 0.06);
    panel.add(line);
  }

  // USB port on the rear panel behind the master section: the built-in audio interface.
  const port = new THREE.Mesh(own(new THREE.BoxGeometry(0.14, 0.06, 0.04)), materials.screen);
  port.position.set(0.92, BACK_HEIGHT * 0.55, -DEPTH / 2 - 0.02);
  group.add(port);
  const portEdge = new THREE.LineSegments(own(new THREE.EdgesGeometry(port.geometry)), materials.edge);
  portEdge.position.copy(port.position);
  group.add(portEdge);
  const usbPort = new THREE.Object3D();
  usbPort.position.set(0.92, BACK_HEIGHT * 0.55, -DEPTH / 2 - 0.05);
  group.add(usbPort);

  const level = (t, seed) => 0.3 + 0.7 * Math.abs(Math.sin(t * (1.1 + seed * 0.13) + seed * 1.7) * Math.cos(t * 0.41 + seed));

  // Animates meters and nudges the faders a little.
  function update(t) {
    faderXs.forEach((x, i) => {
      const home = i < STRIPS ? 0.12 + 0.3 * (0.5 + 0.5 * Math.sin(i * 1.9)) : 0.2;
      caps.setMatrixAt(i, at(x, 0.006, home + 0.025 * Math.sin(t * 0.35 + i)));
    });
    caps.instanceMatrix.needsUpdate = true;

    ledColumns.forEach(({ first, count }, c) => {
      const value = c < STRIPS ? level(t, c) : level(t, 3.3 + c * 0.2);
      const lit = Math.round(value * count);
      for (let j = 0; j < count; j += 1) leds.setColorAt(first + j, j < lit ? LED_ON : LED_OFF);
    });
    leds.instanceColor.needsUpdate = true;
  }
  update(0);

  return {
    group,
    usbPort,
    update,
    dispose() {
      for (const thing of owned) thing.dispose();
    },
  };
}
