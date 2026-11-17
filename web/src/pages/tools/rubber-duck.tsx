import type React from 'react';
import { useEffect, useRef, useState, useCallback } from 'react';
import Layout from '@theme/Layout';
import DownloadCTA from '@site/src/components/DownloadCTA';
import Head from '@docusaurus/Head';
import styles from './rubber-duck.module.css';

// ── Duck dialogue ────────────────────────────────────────────────────────────

const QUACK_VARIATIONS: string[] = [
  'Quack.',
  'Quack quack.',
  '...quack?',
  'QUAAAACK.',
  'Quack. Quack quack. Quack!',
  '*concerned quacking*',
  'Quack. (nodding thoughtfully)',
  'Hmm. Quack.',
  'Ohhh. QUACK.',
  'Quack quack quack quack.',
  '*splashes water thoughtfully* Quack.',
  'Quack! (translation: have you tried turning it off and on again?)',
  '...have you considered it might be an off-by-one error? Quack.',
  'Quack. And what does the error message say?',
  '*waddles closer* Quack.',
  'QUACK. (that\'s your bug right there)',
  'Quack? Quaaack. Quack.',
  '*blinks slowly* Quack.',
  'Interesting. Quack.',
  'Quack quack — wait, did you check the logs?',
  '*tilts head* Quack.',
  'Quack. Have you added a console.log yet?',
  'Quaaaaack... quack. Quack.',
  '*ruffles feathers* Quack.',
  'Quack! Quack quack. (go on...)',
];

const SOLVED_QUACKS: string[] = [
  'QUACK! 🎉 (I knew you\'d get it!)',
  'Quack quack quack! (duck for "I\'m proud of you")',
  'QUAAAACK! You solved it! 🦆',
  '*happy splashing* QUACK QUACK QUACK!',
  'Quack! (the rubber duck method strikes again)',
  'QUUUAAAACK! Go fix it! 🦆🎉',
];

const SOLVE_KEYWORDS = [
  'solved', 'fixed', 'found it', 'got it', 'figured', 'works now',
  'working', 'that\'s it', 'oh!', 'ohh', 'ohhh', 'i see', 'there it is',
  'found the bug', 'found the issue', 'ah ha', 'aha', 'eureka',
];

const DEMO_SCRIPT: { role: 'user' | 'duck'; text: string }[] = [
  { role: 'user',  text: "Okay duck, my function is returning undefined and I have no idea why..." },
  { role: 'duck',  text: 'Quack.' },
  { role: 'user',  text: "It takes an array of users, filters by active status, then maps them to display names." },
  { role: 'duck',  text: 'Quack quack.' },
  { role: 'user',  text: "I call filter() and map() chained together. The logic looks right." },
  { role: 'duck',  text: '*tilts head* Quack.' },
  { role: 'user',  text: "Wait... I'm explaining where the result goes after... oh. Oh no. I never return it from the function." },
  { role: 'duck',  text: 'QUACK. (that\'s your bug right there)' },
  { role: 'user',  text: "I forgot the return statement! The whole function just falls through." },
  { role: 'duck',  text: 'QUAAAACK! You solved it! 🦆' },
];

function pickRandom<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

function looksLikeSolved(text: string): boolean {
  const lower = text.toLowerCase();
  return SOLVE_KEYWORDS.some((kw) => lower.includes(kw));
}

// ── Three.js scene ───────────────────────────────────────────────────────────

interface SceneAPI {
  startSpeaking: () => void;
  stopSpeaking: () => void;
  triggerSolvedDance: () => void;
  jumpToNewAngle: () => void;
  dispose: () => void;
}

async function buildDuckScene(canvas: HTMLCanvasElement): Promise<SceneAPI> {
  const {
    WebGLRenderer, PCFSoftShadowMap, Scene, Color, FogExp2, PerspectiveCamera,
    HemisphereLight, DirectionalLight, PointLight,
    CircleGeometry, RingGeometry, SphereGeometry, CylinderGeometry, BoxGeometry, ConeGeometry,
    MeshStandardMaterial, MeshBasicMaterial,
    Mesh, Group,
  } = await import('three/src/Three.js');

  // ── Renderer ────────────────────────────────────────────────────────────────
  const renderer = new WebGLRenderer({ canvas, antialias: true, alpha: false });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = PCFSoftShadowMap;
  const W = canvas.clientWidth || 700;
  const H = canvas.clientHeight || 420;
  renderer.setSize(W, H, false);

  // ── Scene ────────────────────────────────────────────────────────────────────
  const scene = new Scene();
  scene.background = new Color(0x0ea5e9);
  scene.fog = new FogExp2(0x0ea5e9, 0.04);

  // ── Camera ───────────────────────────────────────────────────────────────────
  const camera = new PerspectiveCamera(50, W / H, 0.1, 200);

  // Camera orbit state
  let camTheta = 0.3;  // horizontal angle
  let camPhi = 0.35;   // vertical angle (elevation)
  let camR = 9;        // radius
  let targetTheta = camTheta;
  let targetPhi = camPhi;
  let targetR = camR;

  const CAM_CONFIGS = [
    { theta: 0.2,  phi: 0.30, r: 9  },
    { theta: 1.1,  phi: 0.45, r: 8  },
    { theta: -0.8, phi: 0.25, r: 10 },
    { theta: 2.2,  phi: 0.5,  r: 8  },
    { theta: -1.5, phi: 0.4,  r: 9  },
    { theta: 3.0,  phi: 0.6,  r: 8.5},
    { theta: -2.5, phi: 0.3,  r: 9  },
  ];
  let camConfigIdx = 0;

  function updateCamera() {
    const x = camR * Math.cos(camPhi) * Math.sin(camTheta);
    const y = camR * Math.sin(camPhi) + 1.5;
    const z = camR * Math.cos(camPhi) * Math.cos(camTheta);
    camera.position.set(x, y, z);
    camera.lookAt(0, 1.8, 0);
  }

  // ── Lighting ─────────────────────────────────────────────────────────────────
  const sky = new HemisphereLight(0x87ceeb, 0x4a90d9, 0.8);
  scene.add(sky);
  const sun = new DirectionalLight(0xfff4cc, 2.0);
  sun.position.set(5, 10, 5);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  sun.shadow.camera.near = 0.1;
  sun.shadow.camera.far = 50;
  sun.shadow.camera.left = -10;
  sun.shadow.camera.right = 10;
  sun.shadow.camera.top = 10;
  sun.shadow.camera.bottom = -10;
  scene.add(sun);
  const rimLight = new PointLight(0xffd700, 1.5, 20);
  rimLight.position.set(-4, 6, -3);
  scene.add(rimLight);

  // ── Water ────────────────────────────────────────────────────────────────────
  const waterGeo = new CircleGeometry(18, 64);
  const waterMat = new MeshStandardMaterial({
    color: 0x1e86c8,
    roughness: 0.1,
    metalness: 0.2,
  });
  const water = new Mesh(waterGeo, waterMat);
  water.rotation.x = -Math.PI / 2;
  water.receiveShadow = true;
  scene.add(water);

  // subtle wave rings
  for (let i = 1; i <= 5; i++) {
    const ring = new Mesh(
      new RingGeometry(i * 1.2, i * 1.2 + 0.06, 64),
      new MeshBasicMaterial({ color: 0x38bdf8, transparent: true, opacity: 0.3 - i * 0.04 }),
    );
    ring.rotation.x = -Math.PI / 2;
    ring.position.y = 0.01;
    scene.add(ring);
  }

  // ── Duck group ───────────────────────────────────────────────────────────────
  const duckGroup = new Group();
  scene.add(duckGroup);

  const YELLOW = new MeshStandardMaterial({ color: 0xf5c842, roughness: 0.6, metalness: 0.1 });
  const ORANGE = new MeshStandardMaterial({ color: 0xf97316, roughness: 0.5 });
  const BLACK  = new MeshStandardMaterial({ color: 0x111827 });
  const WHITE  = new MeshStandardMaterial({ color: 0xffffff });
  const RED    = new MeshStandardMaterial({ color: 0xef4444 });

  // Body — large oval
  const bodyGeo = new SphereGeometry(1.6, 32, 24);
  const body = new Mesh(bodyGeo, YELLOW);
  body.scale.set(1, 0.78, 1.1);
  body.position.y = 0.9;
  body.castShadow = true;
  duckGroup.add(body);

  // Belly (lighter patch)
  const bellyMat = new MeshStandardMaterial({ color: 0xfde68a, roughness: 0.7 });
  const belly = new Mesh(new SphereGeometry(1.1, 24, 20), bellyMat);
  belly.scale.set(0.65, 0.7, 0.4);
  belly.position.set(0, 0.7, 1.1);
  duckGroup.add(belly);

  // Neck
  const neckGeo = new CylinderGeometry(0.55, 0.7, 0.7, 20);
  const neck = new Mesh(neckGeo, YELLOW);
  neck.position.set(0, 2.05, 0.4);
  neck.rotation.x = -0.2;
  neck.castShadow = true;
  duckGroup.add(neck);

  // Head
  const headGeo = new SphereGeometry(0.85, 28, 22);
  const head = new Mesh(headGeo, YELLOW);
  head.position.set(0, 2.9, 0.55);
  head.castShadow = true;
  duckGroup.add(head);

  // Bill group (so we can animate it)
  const billGroup = new Group();
  billGroup.position.set(0, 2.75, 1.35);

  // Upper bill
  const upperBillGeo = new BoxGeometry(0.62, 0.2, 0.55);
  // taper it slightly
  const upperBill = new Mesh(upperBillGeo, ORANGE);
  upperBill.position.y = 0.05;
  billGroup.add(upperBill);

  // Lower bill
  const lowerBillGeo = new BoxGeometry(0.58, 0.14, 0.5);
  const lowerBill = new Mesh(lowerBillGeo, ORANGE);
  lowerBill.position.set(0, -0.08, 0.02);
  billGroup.add(lowerBill);

  // Bill nostrils
  const nostrilGeo = new CircleGeometry(0.055, 8);
  const nostrilMat = new MeshStandardMaterial({ color: 0xc2410c });
  for (const sx of [-0.15, 0.15]) {
    const n = new Mesh(nostrilGeo, nostrilMat);
    n.position.set(sx, 0.12, 0.26);
    billGroup.add(n);
  }

  duckGroup.add(billGroup);

  // Eyes
  const eyeGeo = new SphereGeometry(0.17, 14, 12);
  const eyeL = new Mesh(eyeGeo, BLACK);
  eyeL.position.set(-0.38, 3.05, 1.1);
  duckGroup.add(eyeL);
  const eyeR = eyeL.clone();
  eyeR.position.set(0.38, 3.05, 1.1);
  duckGroup.add(eyeR);

  // Eye gleam
  const gleamGeo = new SphereGeometry(0.055, 8, 8);
  for (const ex of [-0.38, 0.38]) {
    const g = new Mesh(gleamGeo, WHITE);
    g.position.set(ex + 0.06, 3.12, 1.24);
    duckGroup.add(g);
  }

  // Eyelids (half-sphere above each eye for cuteness)
  const lidGeo = new SphereGeometry(0.19, 14, 8, 0, Math.PI * 2, 0, Math.PI / 2);
  const lidMat = new MeshStandardMaterial({ color: 0xe8a900, roughness: 0.7 });
  for (const lx of [-0.38, 0.38]) {
    const lid = new Mesh(lidGeo, lidMat);
    lid.position.set(lx, 3.1, 1.1);
    duckGroup.add(lid);
  }

  // Wing left
  const wingGeo = new SphereGeometry(0.85, 20, 14);
  const wingL = new Mesh(wingGeo, YELLOW);
  wingL.scale.set(0.4, 0.55, 0.9);
  wingL.position.set(-1.45, 1.1, 0.1);
  wingL.rotation.z = 0.3;
  wingL.castShadow = true;
  duckGroup.add(wingL);

  // Wing right
  const wingR = wingL.clone();
  wingR.position.set(1.45, 1.1, 0.1);
  wingR.rotation.z = -0.3;
  duckGroup.add(wingR);

  // Tail
  const tailGeo = new ConeGeometry(0.45, 0.9, 16);
  const tail = new Mesh(tailGeo, YELLOW);
  tail.position.set(0, 1.3, -1.5);
  tail.rotation.x = Math.PI / 2 + 0.4;
  tail.castShadow = true;
  duckGroup.add(tail);

  // Comb (little red crest on head)
  for (let i = 0; i < 3; i++) {
    const comb = new Mesh(new SphereGeometry(0.1 - i * 0.02, 8, 8), RED);
    comb.position.set((i - 1) * 0.15, 3.75 - i * 0.06, 0.4 + i * 0.08);
    duckGroup.add(comb);
  }

  // ── Animation state ──────────────────────────────────────────────────────────
  let speaking = false;
  let solvedDance = false;
  let solvedT = 0;
  let frameId = 0;
  let lastTime = 0;
  let billT = 0;

  // ── Render loop ──────────────────────────────────────────────────────────────
  function animate(time: number) {
    frameId = requestAnimationFrame(animate);
    const dt = Math.min((time - lastTime) / 1000, 0.05);
    lastTime = time;

    // Smooth camera lerp
    const lerpSpeed = 1.5;
    camTheta += (targetTheta - camTheta) * lerpSpeed * dt;
    camPhi   += (targetPhi   - camPhi)   * lerpSpeed * dt;
    camR     += (targetR     - camR)     * lerpSpeed * dt;

    // Slow auto-orbit
    targetTheta += dt * 0.12;

    // Duck gentle bob on water
    const bob = Math.sin(time * 0.0008) * 0.06;
    const sway = Math.sin(time * 0.0005) * 0.03;
    duckGroup.position.y = bob;
    duckGroup.rotation.z = sway;

    // Bill open/close when speaking
    if (speaking) {
      billT += dt * 8;
      const openAmt = Math.abs(Math.sin(billT)) * 0.18;
      lowerBill.position.y = -0.08 - openAmt;
      upperBill.position.y = 0.05 + openAmt * 0.4;
    } else {
      lowerBill.position.y += (-0.08 - lowerBill.position.y) * 8 * dt;
      upperBill.position.y += (0.05  - upperBill.position.y) * 8 * dt;
    }

    // Head look toward "camera" slightly
    const lookAngle = camTheta - Math.PI;
    head.rotation.y += (lookAngle * 0.15 - head.rotation.y) * 2 * dt;
    billGroup.rotation.y = head.rotation.y;

    // Solved dance
    if (solvedDance) {
      solvedT += dt * 3;
      duckGroup.rotation.y = Math.sin(solvedT * 2) * 0.6;
      wingL.rotation.z = 0.3 + Math.sin(solvedT * 3) * 0.5;
      wingR.rotation.z = -0.3 - Math.sin(solvedT * 3) * 0.5;
      duckGroup.position.y = bob + Math.abs(Math.sin(solvedT * 2)) * 0.4;
      if (solvedT > 5) {
        solvedDance = false;
        wingL.rotation.z = 0.3;
        wingR.rotation.z = -0.3;
        duckGroup.rotation.y = 0;
      }
    }

    // Water wave ripple (subtle rotation of the ring geometry via material opacity pulse)
    updateCamera();
    renderer.render(scene, camera);
  }

  frameId = requestAnimationFrame(animate);

  // ── Resize observer ──────────────────────────────────────────────────────────
  const ro = new ResizeObserver(() => {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  });
  ro.observe(canvas);

  // ── Public API ───────────────────────────────────────────────────────────────
  function jumpToNewAngle() {
    camConfigIdx = (camConfigIdx + 1) % CAM_CONFIGS.length;
    const cfg = CAM_CONFIGS[camConfigIdx];
    targetTheta = cfg.theta;
    targetPhi   = cfg.phi;
    targetR     = cfg.r;
  }

  function startSpeaking() { speaking = true; billT = 0; }
  function stopSpeaking()  { speaking = false; }

  function triggerSolvedDance() {
    solvedDance = true;
    solvedT = 0;
  }

  function dispose() {
    cancelAnimationFrame(frameId);
    ro.disconnect();
    renderer.dispose();
  }

  return { startSpeaking, stopSpeaking, triggerSolvedDance, jumpToNewAngle, dispose };
}

// ── React component ───────────────────────────────────────────────────────────

interface Message {
  id: number;
  role: 'user' | 'duck';
  text: string;
  typing?: boolean;
}

let msgId = 0;

export default function RubberDuckPage() {
  const canvasRef  = useRef<HTMLCanvasElement>(null);
  const sceneRef   = useRef<SceneAPI | null>(null);
  const inputRef   = useRef<HTMLTextAreaElement>(null);
  const chatEndRef = useRef<HTMLDivElement>(null);

  const [messages,   setMessages]   = useState<Message[]>([]);
  const [input,      setInput]      = useState('');
  const [duckTyping, setDuckTyping] = useState(false);
  const [solved,     setSolved]     = useState(false);
  const [demoRunning,setDemoRunning]= useState(false);
  const [bubbleText, setBubbleText] = useState('');
  const [bubbleVisible, setBubbleVisible] = useState(false);

  useEffect(() => {
    if (!canvasRef.current) return;
    let mounted = true;
    buildDuckScene(canvasRef.current).then((api) => {
      if (!mounted) { api.dispose(); return; }
      sceneRef.current = api;
    });
    return () => {
      mounted = false;
      sceneRef.current?.dispose();
      sceneRef.current = null;
    };
  }, []);

  // auto-scroll chat
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const showBubble = useCallback((text: string) => {
    setBubbleText(text);
    setBubbleVisible(true);
  }, []);

  const hideBubble = useCallback(() => {
    setBubbleVisible(false);
    setBubbleText('');
  }, []);

  const duckRespond = useCallback(async (userText: string): Promise<void> => {
    const wasSolved = looksLikeSolved(userText);
    const response = wasSolved ? pickRandom(SOLVED_QUACKS) : pickRandom(QUACK_VARIATIONS);
    const delay = 600 + Math.random() * 800;

    setDuckTyping(true);
    sceneRef.current?.startSpeaking();
    sceneRef.current?.jumpToNewAngle();

    await new Promise((r) => setTimeout(r, delay));

    // type the message char by char
    const typingId = ++msgId;
    setMessages((prev) => [...prev, { id: typingId, role: 'duck', text: '', typing: true }]);

    const chars = response.split('');
    showBubble('🦆 ...');

    let built = '';
    for (let i = 0; i < chars.length; i++) {
      built += chars[i];
      const snap = built;
      setMessages((prev) =>
        prev.map((m) => (m.id === typingId ? { ...m, text: snap } : m))
      );
      showBubble(built.length > 28 ? built.slice(0, 28) + '…' : built);
      await new Promise((r) => setTimeout(r, 28 + Math.random() * 20));
    }

    setMessages((prev) =>
      prev.map((m) => (m.id === typingId ? { ...m, typing: false } : m))
    );
    setDuckTyping(false);
    sceneRef.current?.stopSpeaking();

    if (wasSolved) {
      setSolved(true);
      sceneRef.current?.triggerSolvedDance();
    }

    await new Promise((r) => setTimeout(r, 1800));
    hideBubble();
  }, [showBubble, hideBubble]);

  const sendMessage = useCallback(async (text: string) => {
    if (!text.trim() || duckTyping || demoRunning) return;
    const userMsg: Message = { id: ++msgId, role: 'user', text: text.trim() };
    setMessages((prev) => [...prev, userMsg]);
    setInput('');
    await duckRespond(text.trim());
  }, [duckTyping, demoRunning, duckRespond]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      sendMessage(input);
    }
  }, [input, sendMessage]);

  const handleSolvedClick = useCallback(async () => {
    const userMsg: Message = { id: ++msgId, role: 'user', text: "I figured it out! 🎉" };
    setMessages((prev) => [...prev, userMsg]);
    setSolved(true);
    sceneRef.current?.jumpToNewAngle();
    sceneRef.current?.startSpeaking();
    const resp = pickRandom(SOLVED_QUACKS);
    await new Promise((r) => setTimeout(r, 500));

    const typingId = ++msgId;
    setMessages((prev) => [...prev, { id: typingId, role: 'duck', text: '', typing: true }]);
    showBubble('🦆 ...');

    let built = '';
    for (const ch of resp.split('')) {
      built += ch;
      const snap = built;
      setMessages((prev) => prev.map((m) => m.id === typingId ? { ...m, text: snap } : m));
      showBubble(built.length > 28 ? built.slice(0, 28) + '…' : built);
      await new Promise((r) => setTimeout(r, 30));
    }

    setMessages((prev) => prev.map((m) => m.id === typingId ? { ...m, typing: false } : m));
    sceneRef.current?.stopSpeaking();
    sceneRef.current?.triggerSolvedDance();
    await new Promise((r) => setTimeout(r, 1800));
    hideBubble();
  }, [showBubble, hideBubble]);

  const handleReset = useCallback(() => {
    setMessages([]);
    setSolved(false);
    setInput('');
    setDemoRunning(false);
    setBubbleVisible(false);
    setBubbleText('');
    inputRef.current?.focus();
  }, []);

  const runDemo = useCallback(async () => {
    if (demoRunning || duckTyping) return;
    setDemoRunning(true);
    setSolved(false);
    setMessages([]);
    setBubbleVisible(false);

    for (const line of DEMO_SCRIPT) {
      if (line.role === 'user') {
        const id = ++msgId;
        setMessages((prev) => [...prev, { id, role: 'user', text: '', typing: true }]);
        let built = '';
        for (const ch of line.text.split('')) {
          built += ch;
          const snap = built;
          setMessages((prev) => prev.map((m) => m.id === id ? { ...m, text: snap } : m));
          await new Promise((r) => setTimeout(r, 22 + Math.random() * 15));
        }
        setMessages((prev) => prev.map((m) => m.id === id ? { ...m, typing: false } : m));
        await new Promise((r) => setTimeout(r, 400));
      } else {
        // duck response
        const delay = 500 + Math.random() * 600;
        setDuckTyping(true);
        sceneRef.current?.startSpeaking();
        sceneRef.current?.jumpToNewAngle();
        await new Promise((r) => setTimeout(r, delay));

        const id = ++msgId;
        setMessages((prev) => [...prev, { id, role: 'duck', text: '', typing: true }]);
        showBubble('🦆 ...');
        let built = '';
        for (const ch of line.text.split('')) {
          built += ch;
          const snap = built;
          setMessages((prev) => prev.map((m) => m.id === id ? { ...m, text: snap } : m));
          showBubble(built.length > 28 ? built.slice(0, 28) + '…' : built);
          await new Promise((r) => setTimeout(r, 30 + Math.random() * 18));
        }

        setMessages((prev) => prev.map((m) => m.id === id ? { ...m, typing: false } : m));
        setDuckTyping(false);
        sceneRef.current?.stopSpeaking();
        await new Promise((r) => setTimeout(r, 1400));
        hideBubble();
      }
    }

    setSolved(true);
    sceneRef.current?.triggerSolvedDance();
    setDemoRunning(false);
  }, [demoRunning, duckTyping, showBubble, hideBubble]);

  const hasMsgs = messages.length > 0;

  return (
    <Layout
      title="Rubber Duck Debugger"
      description="Explain your bug to a giant rubber duck. You'll probably figure it out yourself."
    >
      <Head>
        <script type="application/ld+json">{JSON.stringify({
          '@context': 'https://schema.org',
          '@type': 'WebApplication',
          name: 'Rubber Duck Debugger',
          description: "Explain your bug to a giant rubber duck. You'll probably figure it out yourself.",
          url: 'https://treq.dev/tools/rubber-duck',
          applicationCategory: 'DeveloperApplication',
          operatingSystem: 'Any',
          isAccessibleForFree: true,
          provider: {'@type': 'Organization', name: 'Treq', url: 'https://treq.dev'},
        })}</script>
      </Head>
      <div className={styles.page}>
        <div className={styles.header}>
          <div className={styles.breadcrumb}>
            <a href="/tools">Tools</a>
            <span> / </span>
            <span>Rubber Duck Debugger</span>
          </div>
          <h1 className={styles.pageTitle}>Rubber Duck Debugger 🦆</h1>
          <p className={styles.pageSubtitle}>
            Explain your bug out loud (well, in text). The duck listens. You'll solve it yourself.
          </p>
        </div>

        <div className={styles.layout}>
          {/* Duck scene */}
          <div className={styles.sceneWrap}>
            <canvas ref={canvasRef} className={styles.canvas} />

            {/* Speech bubble */}
            {bubbleVisible && (
              <div className={styles.bubble}>
                <span className={styles.bubbleText}>{bubbleText}</span>
                <div className={styles.bubbleTail} />
              </div>
            )}

            {/* Duck typing indicator on canvas */}
            {duckTyping && !bubbleVisible && (
              <div className={styles.bubble}>
                <span className={styles.dots}>
                  <span />
                  <span />
                  <span />
                </span>
                <div className={styles.bubbleTail} />
              </div>
            )}
          </div>

          {/* Chat panel */}
          <div className={styles.chatPanel}>
            <div className={styles.chatHeader}>
              <span className={styles.chatTitle}>🦆 Rubber Duck Chat</span>
              <div className={styles.headerActions}>
                {hasMsgs && (
                  <button className={styles.resetBtn} onClick={handleReset}>
                    New session
                  </button>
                )}
                <button
                  className={styles.demoBtn}
                  onClick={runDemo}
                  disabled={demoRunning || duckTyping}
                  title="Watch a demo rubber ducking session"
                >
                  🎲 Demo
                </button>
              </div>
            </div>

            <div className={styles.messages}>
              {messages.length === 0 && (
                <div className={styles.empty}>
                  <div className={styles.emptyDuck}>🦆</div>
                  <p>Describe your bug. The duck is listening.</p>
                  <p className={styles.emptyHint}>
                    Just explaining it out loud (even to a duck) usually reveals the answer.
                  </p>
                </div>
              )}
              {messages.map((msg) => (
                <div
                  key={msg.id}
                  className={`${styles.msg} ${msg.role === 'user' ? styles.msgUser : styles.msgDuck}`}
                >
                  {msg.role === 'duck' && <span className={styles.duckAvatar}>🦆</span>}
                  <div className={styles.msgBubble}>
                    {msg.text || (msg.typing ? <span className={styles.cursor}>▌</span> : '')}
                    {msg.typing && msg.text && <span className={styles.cursor}>▌</span>}
                  </div>
                </div>
              ))}
              <div ref={chatEndRef} />
            </div>

            {!solved ? (
              <div className={styles.inputArea}>
                <textarea
                  ref={inputRef}
                  className={styles.input}
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="Describe your problem... (Enter to send)"
                  disabled={duckTyping || demoRunning}
                  rows={3}
                />
                <div className={styles.inputActions}>
                  <button
                    className={styles.sendBtn}
                    onClick={() => sendMessage(input)}
                    disabled={!input.trim() || duckTyping || demoRunning}
                  >
                    Send
                  </button>
                  {hasMsgs && (
                    <button className={styles.solvedBtn} onClick={handleSolvedClick}>
                      ✅ I solved it!
                    </button>
                  )}
                </div>
              </div>
            ) : (
              <div className={styles.solvedBanner}>
                <span>🎉 Rubber duck debugging: successful!</span>
                <button className={styles.resetBtn} onClick={handleReset}>
                  New session
                </button>
              </div>
            )}
          </div>
        </div>
      </div>
      <DownloadCTA title="Built by Treq" />
    </Layout>
  );
}
