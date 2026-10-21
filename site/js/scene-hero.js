// Hero scene: the BandEar mark rebuilt as glowing amber tubes, floating and tilting toward the
// pointer, with sound-wave rings expanding from its signal arcs. Decorative only.

import * as THREE from "../vendor/three/three.module.min.js";
import { arcToCenter, samplePath } from "./svg-path.js";
import { clamp, damp } from "./motion.js";
import {
  AMBER,
  createRenderer,
  createStage,
  disposeObject,
  glowTexture,
  isPhone,
  prefersReducedMotion,
} from "./three-setup.js";

// The six strokes of assets/brand/bandear-mark.svg (1024 grid, stroke width 44, round caps).
const STROKES = [
  "M520 224 H344 A48 48 0 0 0 296 272 V738 A70 70 0 0 0 366 808 H556 A186 186 0 0 0 692 528",
  "M736 446 Q720 486 678 514",
  "M314 522 C354 482 404 470 476 470 H606",
  "M402 708 H562 A71 71 0 0 0 562 566 H474 A72 72 0 0 0 402 638",
  "M586 302 A92 92 0 0 1 662 386",
  "M584 224 A162 162 0 0 1 738 386",
];
const SIGNAL_STROKES = new Set([4, 5]);

const GRID_CENTER = [517, 516]; // centre of the mark's bounding box
const UNIT = 1 / 240; // world units per grid unit
const STROKE_RADIUS = 22 * UNIT;
const RING_PERIOD = 4.4; // seconds for one ring to travel out and fade
const RING_COUNT = 4;
const STILL_TIME = 1.7; // pose used for the reduced-motion still frame

// Mark + rings bounds in world units, used to frame the camera and centre the composition.
const CONTENT = { minX: -1.15, maxX: 2.55, minY: -1.4, maxY: 2.75 };

const toWorld = ([x, y]) => new THREE.Vector3((x - GRID_CENTER[0]) * UNIT, (GRID_CENTER[1] - y) * UNIT, 0);

// Additive shell that is brightest where it faces the camera: a cheap glow without postprocessing.
function haloMaterial(strength) {
  return new THREE.ShaderMaterial({
    uniforms: { uColor: { value: new THREE.Color(AMBER) }, uStrength: { value: strength } },
    vertexShader: /* glsl */ `
      varying float vFacing;
      void main() {
        vec4 mv = modelViewMatrix * vec4(position, 1.0);
        vFacing = abs(dot(normalize(normalMatrix * normal), normalize(-mv.xyz)));
        gl_Position = projectionMatrix * mv;
      }`,
    fragmentShader: /* glsl */ `
      uniform vec3 uColor;
      uniform float uStrength;
      varying float vFacing;
      void main() {
        gl_FragColor = vec4(uColor, pow(vFacing, 2.5) * uStrength);
        #include <colorspace_fragment>
      }`,
    transparent: true,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
  });
}

// One plane draws every ring: arcs of a circle that grow from the signal arcs and fade out.
function ringsMaterial(inner, outer) {
  return new THREE.ShaderMaterial({
    uniforms: {
      uColor: { value: new THREE.Color(AMBER) },
      uPhase: { value: 0 },
      uInner: { value: inner },
      uOuter: { value: outer },
    },
    vertexShader: /* glsl */ `
      varying vec2 vPos;
      void main() {
        vPos = position.xy;
        gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
      }`,
    fragmentShader: /* glsl */ `
      #define RINGS ${RING_COUNT}
      uniform vec3 uColor;
      uniform float uPhase;
      uniform float uInner;
      uniform float uOuter;
      varying vec2 vPos;
      void main() {
        float r = length(vPos);
        float angle = atan(vPos.y, vPos.x);
        // The signal arcs open toward the upper right (0 to ~90 degrees); soften the sides.
        float mask = smoothstep(-0.6, 0.1, angle) * (1.0 - smoothstep(1.4, 2.1, angle));
        float glow = 0.0;
        for (int i = 0; i < RINGS; i++) {
          float phase = fract(uPhase + float(i) / float(RINGS));
          float radius = mix(uInner, uOuter, phase);
          float width = mix(0.02, 0.055, phase);
          float d = (r - radius) / width;
          float fade = (1.0 - phase) * (1.0 - phase) * smoothstep(0.0, 0.06, phase);
          glow += exp(-d * d) * fade;
        }
        gl_FragColor = vec4(uColor, clamp(glow * mask, 0.0, 1.0) * 0.8);
        #include <colorspace_fragment>
      }`,
    transparent: true,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    side: THREE.DoubleSide,
  });
}

function buildMark(phone) {
  const mark = new THREE.Group();
  const radial = phone ? 10 : 16;
  const core = new THREE.MeshStandardMaterial({
    color: AMBER,
    emissive: 0xff9f1c,
    emissiveIntensity: 0.55,
    roughness: 0.3,
    metalness: 0.2,
  });
  const signalCore = core.clone();
  const halo = haloMaterial(0.32);
  const signalHalo = haloMaterial(0.32);
  const cap = new THREE.SphereGeometry(STROKE_RADIUS, radial, Math.max(8, radial - 4));
  const haloCap = new THREE.SphereGeometry(STROKE_RADIUS * 2.6, 12, 8);

  STROKES.forEach((d, index) => {
    const signal = SIGNAL_STROKES.has(index);
    for (const subpath of samplePath(d, { step: 6 })) {
      const points = subpath.map(toWorld);
      const curve = new THREE.CatmullRomCurve3(points, false, "centripetal");
      curve.arcLengthDivisions = points.length * 4;
      const segments = Math.min(320, points.length * 2);
      mark.add(new THREE.Mesh(new THREE.TubeGeometry(curve, segments, STROKE_RADIUS, radial), signal ? signalCore : core));
      mark.add(
        new THREE.Mesh(
          new THREE.TubeGeometry(curve, Math.min(160, points.length), STROKE_RADIUS * 2.6, 10),
          signal ? signalHalo : halo,
        ),
      );
      // Round caps, like stroke-linecap="round".
      for (const end of [points[0], points[points.length - 1]]) {
        const capMesh = new THREE.Mesh(cap, signal ? signalCore : core);
        capMesh.position.copy(end);
        mark.add(capMesh);
        const haloMesh = new THREE.Mesh(haloCap, signal ? signalHalo : halo);
        haloMesh.position.copy(end);
        mark.add(haloMesh);
      }
    }
  });
  return { mark, signalCore, signalHalo };
}

export function mount(host, { onRendered, onFail } = {}) {
  const canvas = host.querySelector("canvas");
  const phone = isPhone();
  const still = prefersReducedMotion();
  const renderer = createRenderer(canvas);
  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(32, 1, 0.1, 50);
  camera.position.set(0, 0, 8);

  scene.add(new THREE.HemisphereLight(0xfff1d6, 0x0f1416, 1.1));
  const key = new THREE.DirectionalLight(0xffffff, 1.8);
  key.position.set(3, 4, 6);
  scene.add(key);
  const rim = new THREE.PointLight(0xffbf43, 6, 12);
  rim.position.set(-3, -1.5, 2);
  scene.add(rim);

  // Composition: `stage` centres the content, `pivot` (the mark's centre) floats and tilts.
  const stage = new THREE.Group();
  stage.position.set(-(CONTENT.minX + CONTENT.maxX) / 2, -(CONTENT.minY + CONTENT.maxY) / 2, 0);
  scene.add(stage);
  const pivot = new THREE.Group();
  stage.add(pivot);

  const texture = glowTexture();
  const backGlow = new THREE.Sprite(
    new THREE.SpriteMaterial({
      map: texture,
      color: AMBER,
      transparent: true,
      opacity: 0.3,
      blending: THREE.AdditiveBlending,
      depthWrite: false,
    }),
  );
  backGlow.scale.setScalar(3.6); // must fade out before the canvas edges, or a seam shows
  backGlow.position.set(0.15, 0.1, -0.8);
  pivot.add(backGlow);

  const { mark, signalCore, signalHalo } = buildMark(phone);
  pivot.add(mark);

  // Rings centred on the outer signal arc, starting just outside it.
  const signal = arcToCenter([584, 224], 162, 162, 0, false, true, [738, 386]);
  const ringCenter = toWorld([signal.cx, signal.cy]);
  const inner = 162 * UNIT + STROKE_RADIUS * 2.5;
  const outer = 2.75;
  const rings = new THREE.Mesh(new THREE.PlaneGeometry(outer * 2.2, outer * 2.2), ringsMaterial(inner, outer));
  rings.position.copy(ringCenter);
  rings.position.z = -0.05;
  pivot.add(rings);

  // A few drifting motes for depth.
  const moteCount = phone ? 30 : 70;
  const motePositions = new Float32Array(moteCount * 3);
  for (let i = 0; i < moteCount; i += 1) {
    const angle = Math.random() * Math.PI * 2;
    const radius = 1.6 + Math.random() * 1.6;
    motePositions[i * 3] = Math.cos(angle) * radius + 0.6;
    motePositions[i * 3 + 1] = Math.sin(angle) * radius * 0.85 + 0.6;
    motePositions[i * 3 + 2] = (Math.random() - 0.5) * 2.5;
  }
  const moteGeometry = new THREE.BufferGeometry();
  moteGeometry.setAttribute("position", new THREE.BufferAttribute(motePositions, 3));
  const motes = new THREE.Points(
    moteGeometry,
    new THREE.PointsMaterial({
      map: texture,
      color: AMBER,
      size: 0.07,
      transparent: true,
      opacity: 0.55,
      blending: THREE.AdditiveBlending,
      depthWrite: false,
    }),
  );
  stage.add(motes);

  // Pointer (or device tilt on phones) in -1..1, eased each frame.
  const target = { x: 0, y: 0 };
  const tilt = { x: 0, y: 0 };
  const onPointer = (event) => {
    target.x = clamp((event.clientX / window.innerWidth) * 2 - 1, -1, 1);
    target.y = clamp((event.clientY / window.innerHeight) * 2 - 1, -1, 1);
  };
  const onOrientation = (event) => {
    if (event.gamma === null || event.beta === null) return;
    target.x = clamp(event.gamma / 35, -1, 1);
    target.y = clamp((event.beta - 45) / 35, -1, 1);
  };
  window.addEventListener("pointermove", onPointer, { passive: true });
  // iOS gates orientation behind a permission prompt; skip it there rather than ask.
  const orientation =
    phone && "DeviceOrientationEvent" in window && typeof window.DeviceOrientationEvent.requestPermission !== "function";
  if (orientation) window.addEventListener("deviceorientation", onOrientation, { passive: true });

  const stageHandle = createStage({
    host,
    renderer,
    onRendered,
    onFail,
    onResize(width, height) {
      const aspect = width / height;
      const fit = 2 * Math.tan(THREE.MathUtils.degToRad(camera.fov / 2));
      const contentWidth = CONTENT.maxX - CONTENT.minX + 0.4;
      const contentHeight = CONTENT.maxY - CONTENT.minY + 0.4;
      camera.aspect = aspect;
      camera.position.z = Math.max(contentHeight / fit, contentWidth / (fit * aspect));
      camera.updateProjectionMatrix();
    },
    onFrame(elapsed, dt) {
      const t = still ? STILL_TIME : elapsed;
      tilt.x = damp(tilt.x, target.x, 3, dt);
      tilt.y = damp(tilt.y, target.y, 3, dt);
      pivot.rotation.y = Math.sin(t * 0.35) * 0.28 + tilt.x * 0.42;
      pivot.rotation.x = Math.sin(t * 0.27) * 0.06 + tilt.y * 0.28;
      pivot.position.y = Math.sin(t * 0.8) * 0.07;
      motes.rotation.y = t * 0.04;

      const phase = t / RING_PERIOD;
      rings.material.uniforms.uPhase.value = phase;
      // The signal arcs flare each time a new ring leaves them.
      const pulse = Math.exp(-((phase * RING_COUNT) % 1) * 5);
      signalCore.emissiveIntensity = 0.55 + 0.8 * pulse;
      signalHalo.uniforms.uStrength.value = 0.32 + 0.35 * pulse;

      renderer.render(scene, camera);
    },
  });

  return {
    dispose() {
      stageHandle.dispose();
      window.removeEventListener("pointermove", onPointer);
      if (orientation) window.removeEventListener("deviceorientation", onOrientation);
      disposeObject(scene);
      texture.dispose();
      renderer.dispose();
    },
  };
}
