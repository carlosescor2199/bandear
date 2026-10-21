// Page background: a few faint waveform lines and drifting motes behind the content, shifting
// with scroll. Kept at very low contrast so text stays easy to read. Decorative only.

import * as THREE from "../vendor/three/three.module.min.js";
import { particleBudget } from "./motion.js";
import { AMBER, createRenderer, createStage, disposeObject, isPhone, prefersReducedMotion } from "./three-setup.js";

const LINES = [
  { base: -0.62, amp: 34, freq: 4.2, seed: 0.0, alpha: 0.07 },
  { base: -0.2, amp: 48, freq: 3.1, seed: 1.7, alpha: 0.05 },
  { base: 0.24, amp: 28, freq: 5.6, seed: 3.1, alpha: 0.06 },
  { base: 0.64, amp: 40, freq: 3.7, seed: 4.6, alpha: 0.045 },
];
const LINE_VERTICES = 220;

const lineVertex = /* glsl */ `
  attribute float aX;
  uniform vec2 uSize;
  uniform float uTime;
  uniform float uScroll;
  uniform float uBase;
  uniform float uAmp;
  uniform float uFreq;
  uniform float uSeed;
  varying float vEdge;
  void main() {
    float envelope = sin(3.14159265 * (aX * 0.5 + 0.5));
    float wobble = 0.55 + 0.45 * sin(aX * 2.3 + uTime * 0.15 + uSeed);
    float phase = uScroll * 0.0016 + uTime * 0.35;
    float wave = sin(aX * uFreq + phase + uSeed) + 0.35 * sin(aX * uFreq * 2.7 - phase * 1.3 + uSeed * 2.0);
    float y = uBase * uSize.y * 0.5 + sin(uScroll * 0.0008 + uSeed) * 18.0 + uAmp * envelope * wobble * wave;
    vEdge = envelope;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(aX * uSize.x * 0.5, y, 0.0, 1.0);
  }`;

const lineFragment = /* glsl */ `
  uniform vec3 uColor;
  uniform float uAlpha;
  varying float vEdge;
  void main() {
    gl_FragColor = vec4(uColor, uAlpha * (0.25 + 0.75 * vEdge));
    #include <colorspace_fragment>
  }`;

const moteVertex = /* glsl */ `
  attribute vec3 aSeed; // x: column, y: row, z: depth (all 0..1)
  uniform vec2 uSize;
  uniform float uTime;
  uniform float uScroll;
  uniform float uPixelRatio;
  varying float vAlpha;
  varying float vWarm;
  void main() {
    float depth = 0.3 + 0.7 * aSeed.z;
    float span = uSize.y + 40.0;
    float x = (aSeed.x - 0.5) * uSize.x + sin(uTime * 0.1 + aSeed.y * 20.0) * 12.0;
    // Nearer motes drift and scroll faster (parallax); wrap around the viewport.
    float travel = aSeed.y * span + uScroll * 0.25 * depth + uTime * 6.0 * depth;
    float y = mod(travel, span) - span * 0.5;
    gl_PointSize = (1.0 + 2.2 * depth) * uPixelRatio;
    vAlpha = (0.1 + 0.22 * depth) * (0.6 + 0.4 * sin(uTime * 0.8 + aSeed.x * 40.0));
    vWarm = step(0.78, fract(aSeed.x * 7.0 + aSeed.z * 3.0));
    gl_Position = projectionMatrix * modelViewMatrix * vec4(x, y, 0.0, 1.0);
  }`;

const moteFragment = /* glsl */ `
  uniform vec3 uCool;
  uniform vec3 uWarm;
  varying float vAlpha;
  varying float vWarm;
  void main() {
    float d = length(gl_PointCoord - 0.5);
    float alpha = smoothstep(0.5, 0.1, d) * vAlpha;
    gl_FragColor = vec4(mix(uCool, uWarm, vWarm), alpha);
    #include <colorspace_fragment>
  }`;

export function mount(host, { onRendered, onFail } = {}) {
  const canvas = host.querySelector("canvas");
  const renderer = createRenderer(canvas, { maxPixelRatio: 1 }); // soft content: 1x is plenty
  const scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, -10, 10);
  const size = new THREE.Vector2(1, 1);
  const shared = {
    uSize: { value: size },
    uTime: { value: 0 },
    uScroll: { value: 0 },
  };

  // Waveform lines: x is a normalised attribute, the shader does the rest.
  const xs = new Float32Array(LINE_VERTICES);
  for (let i = 0; i < LINE_VERTICES; i += 1) xs[i] = (i / (LINE_VERTICES - 1)) * 2 - 1;
  const lineGeometry = new THREE.BufferGeometry();
  lineGeometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(LINE_VERTICES * 3), 3));
  lineGeometry.setAttribute("aX", new THREE.BufferAttribute(xs, 1));
  for (const line of LINES) {
    const material = new THREE.ShaderMaterial({
      uniforms: {
        ...shared,
        uBase: { value: line.base },
        uAmp: { value: line.amp },
        uFreq: { value: line.freq },
        uSeed: { value: line.seed },
        uAlpha: { value: line.alpha },
        uColor: { value: new THREE.Color(AMBER) },
      },
      vertexShader: lineVertex,
      fragmentShader: lineFragment,
      transparent: true,
      depthWrite: false,
    });
    const object = new THREE.Line(lineGeometry, material);
    object.frustumCulled = false;
    scene.add(object);
  }

  // Motes.
  const count = particleBudget(window.innerWidth, isPhone() ? 110 : 180);
  const seeds = new Float32Array(count * 3);
  for (let i = 0; i < seeds.length; i += 1) seeds[i] = Math.random();
  const moteGeometry = new THREE.BufferGeometry();
  moteGeometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(count * 3), 3));
  moteGeometry.setAttribute("aSeed", new THREE.BufferAttribute(seeds, 3));
  const motes = new THREE.Points(
    moteGeometry,
    new THREE.ShaderMaterial({
      uniforms: {
        ...shared,
        uPixelRatio: { value: renderer.getPixelRatio() },
        uCool: { value: new THREE.Color(0xa9b7b6) },
        uWarm: { value: new THREE.Color(AMBER) },
      },
      vertexShader: moteVertex,
      fragmentShader: moteFragment,
      transparent: true,
      depthWrite: false,
    }),
  );
  motes.frustumCulled = false;
  scene.add(motes);

  const still = prefersReducedMotion();
  const stageHandle = createStage({
    host,
    renderer,
    onRendered,
    onFail,
    alwaysOn: true,
    fps: 30, // slow, soft motion: half rate is indistinguishable and halves the cost
    onResize(width, height) {
      size.set(width, height);
      camera.left = -width / 2;
      camera.right = width / 2;
      camera.top = height / 2;
      camera.bottom = -height / 2;
      camera.updateProjectionMatrix();
    },
    onFrame(elapsed) {
      shared.uTime.value = still ? 8 : elapsed;
      shared.uScroll.value = window.scrollY;
      renderer.render(scene, camera);
    },
  });

  return {
    dispose() {
      stageHandle.dispose();
      disposeObject(scene);
      renderer.dispose();
    },
  };
}
