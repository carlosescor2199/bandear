// "How it works" scene: a mixing console (with its built-in USB audio interface), a laptop and
// three phones as low-poly shapes, with amber audio packets flowing along the USB cable to the
// laptop and on over Wi-Fi to each phone. The camera drifts with the
// section's scroll progress. Labels are HTML, positioned from 3D anchors. Decorative only: the
// accessible SVG diagram stays in the page for screen readers.

import * as THREE from "../vendor/three/three.module.min.js";
import { t } from "./i18n.js";
import { buildMixer } from "./model-mixer.js";
import { damp, lerp, particleBudget, sectionProgress } from "./motion.js";
import {
  AMBER,
  createStage,
  disposeObject,
  isPhone,
  prefersReducedMotion,
  withRenderer,
} from "./three-setup.js";

const BODY = 0x223031;
const SCREEN = 0x0b1012;
const FOV = 30;
const LOOKUP_STEPS = 96;

// Two arrangements: a left-to-right row for wide canvases and a top-to-bottom stack for phones.
// Positions/rotations are world units; label offsets are relative to each device's position.
const LAYOUTS = {
  wide: {
    bounds: { minX: -5.8, maxX: 5.5, minY: -2.0, maxY: 1.7 }, // right side leaves room for phone labels
    orbit: { azimuth: [-0.34, 0.3], elevation: [0.38, 0.16] },
    mixer: { position: [-4.0, -0.8, 0.3], rotationX: 0.32, rotationY: 0.3, scale: 1.15, label: [0, -0.6, 1.0], align: "below" },
    laptop: { position: [-0.7, -0.85, 0], rotationY: 0.1, label: [0, -0.32, 0.95], align: "below" },
    phones: [
      { position: [3.45, 1.3, -0.3], rotationY: -0.35, label: [0.5, 0, 0], align: "right" },
      { position: [3.8, -0.05, 0.1], rotationY: -0.35, label: [0.5, 0, 0], align: "right" },
      { position: [3.45, -1.4, 0.5], rotationY: -0.35, label: [0.5, 0, 0], align: "right" },
    ],
  },
  tall: {
    bounds: { minX: -2.3, maxX: 2.3, minY: -3.9, maxY: 4.45 },
    orbit: { azimuth: [-0.26, 0.26], elevation: [0.3, 0.12] },
    mixer: { position: [0, 3.25, 0], rotationX: 0.5, rotationY: 0, label: [0, -0.5, 0.9], align: "below" },
    laptop: { position: [0, 0.35, 0], rotationY: 0, label: [0, -0.3, 0.95], align: "below" },
    phones: [
      { position: [-1.55, -2.45, 0], rotationY: 0.22, label: [0, -0.82, 0], align: "below" },
      { position: [0, -2.6, 0.3], rotationY: 0, label: [0, -0.82, 0], align: "below" },
      { position: [1.55, -2.45, 0], rotationY: -0.22, label: [0, -0.82, 0], align: "below" },
    ],
  },
};
// Dictionary keys (see i18n.js); the labels carry data-i18n so a language switch rewrites them.
const LABELS = ["diagram.mixer", "diagram.desktop", "diagram.singer", "diagram.drummer", "diagram.keys"];

function particleMaterial() {
  return new THREE.ShaderMaterial({
    uniforms: { uColor: { value: new THREE.Color(AMBER) }, uScale: { value: 400 } },
    vertexShader: /* glsl */ `
      attribute float aFade;
      attribute float aSize;
      uniform float uScale;
      varying float vFade;
      void main() {
        vFade = aFade;
        vec4 mv = modelViewMatrix * vec4(position, 1.0);
        gl_PointSize = aSize * uScale / -mv.z;
        gl_Position = projectionMatrix * mv;
      }`,
    fragmentShader: /* glsl */ `
      uniform vec3 uColor;
      varying float vFade;
      void main() {
        float d = length(gl_PointCoord - 0.5);
        float alpha = smoothstep(0.5, 0.0, d);
        vec3 color = mix(uColor, vec3(1.0, 0.96, 0.86), smoothstep(0.16, 0.0, d));
        gl_FragColor = vec4(color, alpha * alpha * vFade);
        #include <colorspace_fragment>
      }`,
    transparent: true,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
  });
}

export function mount(host, callbacks = {}) {
  const labelLayer = host.querySelector(".flow-labels");
  if (!labelLayer) throw new Error("the flow scene needs a .flow-labels layer");
  return withRenderer(host, {}, (renderer) => build(host, renderer, labelLayer, callbacks));
}

function build(host, renderer, labelLayer, { onRendered, onFail }) {
  const section = host.closest("section") ?? host;
  const phone = isPhone();
  const still = prefersReducedMotion();

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 80);

  scene.add(new THREE.HemisphereLight(0xe9f1ef, 0x0b1012, 1.2));
  const key = new THREE.DirectionalLight(0xffffff, 1.6);
  key.position.set(-3, 6, 8);
  scene.add(key);
  const warm = new THREE.PointLight(AMBER, 10, 9);
  warm.position.set(0, 1.5, 2.5);
  scene.add(warm);

  const materials = {
    body: new THREE.MeshStandardMaterial({ color: BODY, roughness: 0.55, metalness: 0.25 }),
    screen: new THREE.MeshBasicMaterial({ color: SCREEN }),
    edge: new THREE.LineBasicMaterial({ color: AMBER, transparent: true, opacity: 0.55 }),
    lit: new THREE.MeshBasicMaterial({ color: AMBER }),
    cable: new THREE.MeshStandardMaterial({ color: 0x55625f, roughness: 0.6 }), // USB cable
    wifi: new THREE.LineDashedMaterial({ color: AMBER, dashSize: 0.07, gapSize: 0.13, transparent: true, opacity: 0.35 }),
  };

  // A box with amber edge lines.
  const block = (width, height, depth) => {
    const geometry = new THREE.BoxGeometry(width, height, depth);
    const group = new THREE.Group();
    group.add(new THREE.Mesh(geometry, materials.body));
    group.add(new THREE.LineSegments(new THREE.EdgesGeometry(geometry), materials.edge));
    return group;
  };
  const anchor = (parent, x, y, z) => {
    const point = new THREE.Object3D();
    point.position.set(x, y, z);
    parent.add(point);
    return point;
  };
  // Bars that grow upward from their base (meters and faders).
  const barGeometry = new THREE.BoxGeometry(1, 1, 0.01).translate(0, 0.5, 0);
  const meters = [];
  const addBar = (parent, x, y, z, width, maxHeight, speed, offset) => {
    const bar = new THREE.Mesh(barGeometry, materials.lit);
    bar.position.set(x, y, z);
    bar.scale.set(width, maxHeight, 1);
    parent.add(bar);
    meters.push({ bar, maxHeight, speed, offset });
  };

  // Mixing console with a built-in USB audio interface (port on the rear panel).
  const mixer = buildMixer(materials);
  scene.add(mixer.group);

  // Laptop: base plus a tilted screen showing moving faders.
  const laptop = new THREE.Group();
  const base = block(2.4, 0.1, 1.6);
  base.position.y = 0.05;
  laptop.add(base);
  const hinge = new THREE.Group();
  hinge.position.set(0, 0.1, -0.78);
  hinge.rotation.x = -0.2;
  laptop.add(hinge);
  const lid = block(2.4, 1.5, 0.06);
  lid.position.y = 0.75;
  hinge.add(lid);
  const face = new THREE.Mesh(new THREE.PlaneGeometry(2.2, 1.3), materials.screen);
  face.position.set(0, 0.75, 0.032);
  hinge.add(face);
  for (let i = 0; i < 8; i += 1) {
    addBar(hinge, -0.84 + i * 0.24, 0.24, 0.036, 0.09, 0.95, 1.1 + (i % 3) * 0.35, i * 1.3);
  }
  const laptopInLeft = anchor(laptop, -1.24, 0.06, 0.2); // USB socket, left side (wide layout)
  const laptopInRight = anchor(laptop, 1.24, 0.06, 0.2); // USB socket, right side (tall layout)
  // USB plugs on the laptop side; only the one the current layout uses is shown.
  const plugs = [laptopInLeft, laptopInRight].map((socket) => {
    const plug = block(0.12, 0.05, 0.12);
    plug.position.copy(socket.position);
    laptop.add(plug);
    return plug;
  });
  const laptopOut = anchor(hinge, 0, 1.55, 0);
  const laptopFront = anchor(laptop, 0, 0.08, 0.85); // tall layout: Wi-Fi leaves from the front edge
  scene.add(laptop);

  // Phones: slim slabs with a mini mix on screen.
  const phones = [0, 1, 2].map((index) => {
    const device = block(0.62, 1.2, 0.08);
    const screen = new THREE.Mesh(new THREE.PlaneGeometry(0.52, 1.02), materials.screen);
    screen.position.z = 0.041;
    device.add(screen);
    for (let i = 0; i < 3; i += 1) {
      addBar(device, -0.14 + i * 0.14, -0.4, 0.043, 0.06, 0.55, 0.7 + i * 0.25, index * 2 + i);
    }
    scene.add(device);
    return { device, input: anchor(device, 0, 0.18, 0.06) };
  });

  // HTML labels (decorative; the hidden SVG keeps the accessible description).
  const labels = LABELS.map((key) => {
    const element = document.createElement("span");
    element.className = "flow-label";
    element.dataset.i18n = key;
    element.textContent = t(key);
    labelLayer.append(element);
    return { element, last: "" };
  });
  const devices = [mixer.group, laptop, ...phones.map((p) => p.device)];

  // Paths and particles are rebuilt whenever the layout switches.
  const paths = new THREE.Group();
  scene.add(paths);
  const lookups = [];
  const perPath = particleBudget(window.innerWidth, phone ? 22 : 34);
  const total = perPath * 4;
  const positions = new Float32Array(total * 3);
  const fades = new Float32Array(total);
  const sizes = new Float32Array(total);
  const seeds = new Float32Array(total);
  for (let i = 0; i < total; i += 1) {
    seeds[i] = ((i % perPath) + Math.random() * 0.6) / perPath;
    sizes[i] = 0.16 + Math.random() * 0.14;
  }
  const particleGeometry = new THREE.BufferGeometry();
  particleGeometry.setAttribute("position", new THREE.BufferAttribute(positions, 3).setUsage(THREE.DynamicDrawUsage));
  particleGeometry.setAttribute("aFade", new THREE.BufferAttribute(fades, 1).setUsage(THREE.DynamicDrawUsage));
  particleGeometry.setAttribute("aSize", new THREE.BufferAttribute(sizes, 1));
  const particles = new THREE.Points(particleGeometry, particleMaterial());
  particles.frustumCulled = false;
  scene.add(particles);

  let layout = null;
  let layoutName = "";
  const center = new THREE.Vector3();
  let distance = 12;

  function applyLayout(name) {
    layoutName = name;
    layout = LAYOUTS[name];
    const place = (object, spec) => {
      object.position.set(...spec.position);
      object.rotation.set(spec.rotationX ?? 0, spec.rotationY, 0, "YXZ");
      object.scale.setScalar(spec.scale ?? 1);
    };
    place(mixer.group, layout.mixer);
    place(laptop, layout.laptop);
    phones.forEach((p, i) => place(p.device, layout.phones[i]));
    scene.updateMatrixWorld(true);

    for (const child of [...paths.children]) {
      child.geometry.dispose();
      paths.remove(child);
    }
    lookups.length = 0;
    const world = (object) => object.getWorldPosition(new THREE.Vector3());

    // USB cable from the console's rear port (its built-in audio interface) to the laptop's side
    // socket: out of the back, then round into the socket. Audio packets ride along it.
    const tall = name === "tall";
    const socket = tall ? laptopInRight : laptopInLeft;
    plugs[0].visible = !tall;
    plugs[1].visible = tall;
    const a = world(mixer.usbPort);
    const b = world(socket);
    const back = new THREE.Vector3(0, 0, -1).transformDirection(mixer.group.matrixWorld);
    const side = new THREE.Vector3(tall ? 1 : -1, 0, 0).transformDirection(laptop.matrixWorld);
    const c1 = a.clone().addScaledVector(back, 0.5).add(new THREE.Vector3(0, tall ? -0.2 : 1.1, 0));
    const c2 = b.clone().addScaledVector(side, 1.0).add(new THREE.Vector3(0, tall ? 0.6 : 0.5, 0.2));
    const cable = new THREE.CubicBezierCurve3(a, c1, c2, b);
    paths.add(new THREE.Mesh(new THREE.TubeGeometry(cable, 64, 0.035, 8), materials.cable));
    lookups.push(cable.getSpacedPoints(LOOKUP_STEPS));

    // Wi-Fi arcs to each phone: from the top of the screen (wide) or the front edge (tall).
    const start = world(tall ? laptopFront : laptopOut);
    for (const { input } of phones) {
      const end = world(input);
      const lift = tall ? new THREE.Vector3(0, 0.15, 0.7) : new THREE.Vector3(0, 0.7, 0.5);
      const c1 = start.clone().lerp(end, 0.3).add(lift);
      const c2 = start.clone().lerp(end, 0.7).add(lift.clone().multiplyScalar(0.6));
      const curve = new THREE.CubicBezierCurve3(start, c1, c2, end);
      const line = new THREE.Line(new THREE.BufferGeometry().setFromPoints(curve.getPoints(64)), materials.wifi);
      line.computeLineDistances();
      paths.add(line);
      lookups.push(curve.getSpacedPoints(LOOKUP_STEPS));
    }

    const { minX, maxX, minY, maxY } = layout.bounds;
    center.set((minX + maxX) / 2, (minY + maxY) / 2, 0);
  }

  function fitCamera(width, height) {
    const { minX, maxX, minY, maxY } = layout.bounds;
    const fit = 2 * Math.tan(THREE.MathUtils.degToRad(FOV / 2));
    const aspect = width / height;
    distance = Math.max((maxY - minY) / fit, (maxX - minX) / (fit * aspect)) * 0.98 + 0.6;
    camera.aspect = aspect;
    camera.updateProjectionMatrix();
    particles.material.uniforms.uScale.value = (height * renderer.getPixelRatio()) / fit;
  }

  const point = new THREE.Vector3();
  function sampleLookup(lookup, t) {
    const x = t * LOOKUP_STEPS;
    const i = Math.min(LOOKUP_STEPS - 1, Math.floor(x));
    return point.copy(lookup[i]).lerp(lookup[i + 1], x - i);
  }

  function updateLabels(width, height) {
    devices.forEach((device, i) => {
      const spec = i === 0 ? layout.mixer : i === 1 ? layout.laptop : layout.phones[i - 2];
      point.set(...spec.label).add(device.position).project(camera);
      const x = Math.round(((point.x + 1) / 2) * width);
      const y = Math.round(((1 - point.y) / 2) * height);
      const shift = spec.align === "right" ? "translate(0, -50%)" : "translate(-50%, 0)";
      const transform = `translate(${x}px, ${y}px) ${shift}`;
      if (labels[i].last !== transform) {
        labels[i].last = transform;
        labels[i].element.style.transform = transform;
      }
    });
  }

  let progress = 0.5;
  const view = { width: 0, height: 0 };
  const stageHandle = createStage({
    host,
    renderer,
    onRendered,
    onFail,
    onResize(width, height) {
      const name = width / height < 1.2 ? "tall" : "wide";
      if (name !== layoutName) applyLayout(name);
      view.width = width;
      view.height = height;
      fitCamera(width, height);
    },
    onFrame(elapsed, dt) {
      const t = still ? 2 : elapsed;
      const rect = section.getBoundingClientRect();
      const target = still ? 0.5 : sectionProgress(rect.top, rect.height, window.innerHeight);
      progress = dt === 0 ? target : damp(progress, target, 4, dt);

      // Camera orbits the scene centre as the section scrolls through the viewport.
      const azimuth = lerp(layout.orbit.azimuth[0], layout.orbit.azimuth[1], progress);
      const elevation = lerp(layout.orbit.elevation[0], layout.orbit.elevation[1], progress);
      camera.position.set(
        center.x + distance * Math.sin(azimuth) * Math.cos(elevation),
        center.y + distance * Math.sin(elevation),
        center.z + distance * Math.cos(azimuth) * Math.cos(elevation),
      );
      camera.lookAt(center);
      camera.updateMatrixWorld();

      // Audio packets: the cable runs a little faster than the Wi-Fi hops.
      for (let i = 0; i < total; i += 1) {
        const path = Math.floor(i / perPath);
        const speed = path === 0 ? 0.3 : 0.24 + path * 0.015;
        const u = (seeds[i] + t * speed) % 1;
        const p = sampleLookup(lookups[path], u);
        positions[i * 3] = p.x;
        positions[i * 3 + 1] = p.y;
        positions[i * 3 + 2] = p.z;
        fades[i] = Math.min(1, u / 0.08, (1 - u) / 0.12);
      }
      particleGeometry.attributes.position.needsUpdate = true;
      particleGeometry.attributes.aFade.needsUpdate = true;

      // Meters and faders move with the "music".
      for (const { bar, maxHeight, speed, offset } of meters) {
        const level = 0.35 + 0.65 * Math.abs(Math.sin(t * speed + offset) * Math.cos(t * speed * 0.37 + offset));
        bar.scale.y = maxHeight * level;
      }
      mixer.update(t);

      renderer.render(scene, camera);
      updateLabels(view.width, view.height);
    },
  });

  return {
    dispose() {
      stageHandle.dispose();
      for (const { element } of labels) element.remove();
      disposeObject(scene);
      mixer.dispose();
      barGeometry.dispose();
      renderer.dispose();
    },
  };
}
