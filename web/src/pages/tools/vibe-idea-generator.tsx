import React, { useEffect, useRef, useState, useCallback } from 'react';
import Layout from '@theme/Layout';
import DownloadCTA from '@site/src/components/DownloadCTA';
import Head from '@docusaurus/Head';
import styles from './vibe-idea-generator.module.css';

// ── Idea generation data ──────────────────────────────────────────────────────

const ADJECTIVES = [
  'AI-powered', 'cloud-native', 'blockchain-enabled', 'quantum-resistant',
  'serverless', 'hyper-scalable', 'zero-latency', 'event-driven',
  'self-healing', 'context-aware', 'privacy-first', 'composable',
  'real-time', 'headless', 'edge-optimized', 'multi-tenant',
  'ML-enhanced', 'LLM-augmented', 'autonomous', 'intelligent',
  'distributed', 'micro-frontends-based', 'low-code', 'no-code',
  'DevSecOps-native', 'GitOps-powered', 'container-first', 'Kubernetes-native',
  'data-mesh-aligned', 'API-first',
];

const VERBS = [
  'disrupting', 'reimagining', 'democratizing', 'supercharging',
  'unlocking', 'turbochargning', 'decentralizing', 'platformizing',
  'productizing', 'gamifying', 'synergizing', 'orchestrating',
  'operationalizing', 'monetizing', 'scale-pilling', 'shipping',
  'dogfooding', 'open-sourcing', 'compositing', 'vibe-coding',
];

const PRODUCTS = [
  'developer experience', 'the software supply chain', 'incident response',
  'code review workflows', 'technical debt management', 'knowledge graphs',
  'feature flag systems', 'observability pipelines', 'CI/CD orchestration',
  'API governance', 'data lineage tracking', 'secrets management',
  'dependency auditing', 'monorepo tooling', 'trunk-based development',
  'inner-source adoption', 'platform engineering', 'shift-left security',
  'service mesh infrastructure', 'developer portals',
  'AI agent collaboration', 'multi-model inference routing', 'prompt versioning',
  'RAG pipelines', 'LLM evaluation harnesses', 'vector database migrations',
];

const SUFFIXES = [
  'at scale', 'for the agentic era', 'with zero-shot prompting',
  'via semantic diffing', 'through composable primitives',
  'using graph-based reasoning', 'with full audit trails',
  'powered by RAG', 'on the edge', 'across hybrid clouds',
  'for the post-AGI workforce', 'through GitOps principles',
  'with DX in mind', 'leveraging async everything',
  'for 10x engineers', 'beyond the monolith',
  'with an AI copilot', 'end-to-end', 'natively in the browser',
  'without the boilerplate',
];

function pick<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

function generateIdea(): string {
  return `${pick(ADJECTIVES)} platform ${pick(VERBS)} ${pick(PRODUCTS)} ${pick(SUFFIXES)}`;
}

// ── Three.js scene ────────────────────────────────────────────────────────────

type AnimPhase =
  | 'idle'
  | 'windup'
  | 'throw'
  | 'flying'
  | 'stuck'
  | 'unfolding'
  | 'reading';

interface SceneHandles {
  trigger: () => void;
  dispose: () => void;
}

async function buildScene(canvas: HTMLCanvasElement, onReveal: (idea: string) => void): Promise<SceneHandles> {
  const {
    WebGLRenderer, PCFSoftShadowMap, Scene, Color, Fog, PerspectiveCamera,
    AmbientLight, DirectionalLight, PointLight,
    PlaneGeometry, BoxGeometry, SphereGeometry, IcosahedronGeometry, ConeGeometry, CylinderGeometry,
    MeshStandardMaterial, Mesh, Group,
    DoubleSide, MathUtils, Vector3,
  } = await import('three/src/Three.js');

  // ── Renderer ────────────────────────────────────────────────────────────────
  const renderer = new WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = PCFSoftShadowMap;

  const W = canvas.clientWidth || 700;
  const H = canvas.clientHeight || 380;
  renderer.setSize(W, H, false);

  // ── Scene + camera ───────────────────────────────────────────────────────────
  const scene = new Scene();
  scene.background = new Color(0x0f172a);
  scene.fog = new Fog(0x0f172a, 18, 32);

  const camera = new PerspectiveCamera(55, W / H, 0.1, 100);
  camera.position.set(0, 3, 9);
  camera.lookAt(0, 1.5, 0);

  // ── Lighting ─────────────────────────────────────────────────────────────────
  const ambient = new AmbientLight(0xffffff, 0.4);
  scene.add(ambient);

  const sun = new DirectionalLight(0xffd9b3, 1.6);
  sun.position.set(4, 8, 4);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  scene.add(sun);

  const fill = new PointLight(0x6366f1, 1.2, 20);
  fill.position.set(-4, 4, 2);
  scene.add(fill);

  // ── Floor ─────────────────────────────────────────────────────────────────────
  const floorGeo = new PlaneGeometry(40, 40);
  const floorMat = new MeshStandardMaterial({ color: 0x1e293b });
  const floor = new Mesh(floorGeo, floorMat);
  floor.rotation.x = -Math.PI / 2;
  floor.receiveShadow = true;
  scene.add(floor);

  // ── Wall ─────────────────────────────────────────────────────────────────────
  const wallGeo = new PlaneGeometry(20, 10);
  const wallMat = new MeshStandardMaterial({ color: 0x334155 });
  const wall = new Mesh(wallGeo, wallMat);
  wall.position.set(0, 4, -8);
  wall.receiveShadow = true;
  scene.add(wall);

  // wall pin strip (subtle horizontal band)
  const stripGeo = new PlaneGeometry(20, 0.05);
  const stripMat = new MeshStandardMaterial({ color: 0x475569 });
  const strip = new Mesh(stripGeo, stripMat);
  strip.position.set(0, 2.6, -7.98);
  scene.add(strip);

  // ── Couch ─────────────────────────────────────────────────────────────────────
  const COUCH_COLOR = 0x7c3aed;
  const cushionMat = new MeshStandardMaterial({ color: COUCH_COLOR });
  const darkMat = new MeshStandardMaterial({ color: 0x4c1d95 });

  // seat
  const seat = new Mesh(new BoxGeometry(3.2, 0.5, 1.4), cushionMat);
  seat.position.set(0, 0.5, 1.5);
  seat.castShadow = true;
  seat.receiveShadow = true;
  scene.add(seat);

  // back
  const back = new Mesh(new BoxGeometry(3.2, 1.2, 0.4), cushionMat);
  back.position.set(0, 1.35, 2.1);
  back.castShadow = true;
  scene.add(back);

  // left arm
  const armL = new Mesh(new BoxGeometry(0.4, 0.9, 1.4), darkMat);
  armL.position.set(-1.8, 0.7, 1.5);
  armL.castShadow = true;
  scene.add(armL);

  // right arm
  const armR = armL.clone();
  armR.position.set(1.8, 0.7, 1.5);
  scene.add(armR);

  // legs
  const legGeo = new BoxGeometry(0.15, 0.35, 0.15);
  const legMat = new MeshStandardMaterial({ color: 0x1e293b });
  [[-1.4, 0.17, 0.9], [1.4, 0.17, 0.9], [-1.4, 0.17, 2.1], [1.4, 0.17, 2.1]].forEach(([x, y, z]) => {
    const leg = new Mesh(legGeo, legMat);
    leg.position.set(x, y, z);
    scene.add(leg);
  });

  // ── Potato body ───────────────────────────────────────────────────────────────
  const potatoGroup = new Group();

  // body — squashed sphere
  const bodyGeo = new SphereGeometry(0.55, 20, 16);
  const bodyMat = new MeshStandardMaterial({ color: 0xc9a96e, roughness: 0.9, metalness: 0.0 });
  const body = new Mesh(bodyGeo, bodyMat);
  body.scale.set(1, 0.85, 1);
  body.castShadow = true;
  potatoGroup.add(body);

  // eyes
  const eyeGeo = new SphereGeometry(0.07, 8, 8);
  const eyeMat = new MeshStandardMaterial({ color: 0x1e293b });
  const eyeL = new Mesh(eyeGeo, eyeMat);
  eyeL.position.set(-0.18, 0.15, 0.5);
  potatoGroup.add(eyeL);
  const eyeR = eyeL.clone();
  eyeR.position.set(0.18, 0.15, 0.5);
  potatoGroup.add(eyeR);

  // pupils (white dot)
  const pupilGeo = new SphereGeometry(0.025, 6, 6);
  const pupilMat = new MeshStandardMaterial({ color: 0xffffff });
  const pupilL = new Mesh(pupilGeo, pupilMat);
  pupilL.position.set(-0.155, 0.17, 0.565);
  potatoGroup.add(pupilL);
  const pupilR = pupilL.clone();
  pupilR.position.set(0.205, 0.17, 0.565);
  potatoGroup.add(pupilR);

  // mouth (arc made of small spheres)
  for (let i = 0; i < 7; i++) {
    const t = (i / 6) * Math.PI;
    const mx = Math.cos(t) * 0.18;
    const my = -0.12 - Math.sin(t) * 0.07;
    const sm = new Mesh(new SphereGeometry(0.025, 6, 6), eyeMat);
    sm.position.set(mx, my, 0.545);
    potatoGroup.add(sm);
  }

  // arm (right, throwing)
  const armGeo = new CylinderGeometry(0.07, 0.06, 0.55, 8);
  const armMat = new MeshStandardMaterial({ color: 0xb8860b });
  const throwArm = new Mesh(armGeo, armMat);
  throwArm.position.set(0.58, 0.0, 0.1);
  throwArm.rotation.z = -Math.PI / 4;
  throwArm.castShadow = true;
  potatoGroup.add(throwArm);

  // left arm (resting)
  const restArm = new Mesh(armGeo, armMat);
  restArm.position.set(-0.58, -0.1, 0.1);
  restArm.rotation.z = Math.PI / 4;
  scene.add(potatoGroup); // add before positioning

  potatoGroup.add(restArm);
  potatoGroup.position.set(0, 1.28, 1.1);

  // ── Crumpled paper ball ───────────────────────────────────────────────────────
  const paperGroup = new Group();

  // main ball
  const paperGeo = new IcosahedronGeometry(0.18, 1);
  // slightly deform for crumpled look
  const posArr = paperGeo.attributes.position.array as Float32Array;
  for (let i = 0; i < posArr.length; i += 3) {
    posArr[i] += (Math.random() - 0.5) * 0.04;
    posArr[i + 1] += (Math.random() - 0.5) * 0.04;
    posArr[i + 2] += (Math.random() - 0.5) * 0.04;
  }
  paperGeo.computeVertexNormals();

  const paperMat = new MeshStandardMaterial({ color: 0xf1f5f9, roughness: 0.95 });
  const paperBall = new Mesh(paperGeo, paperMat);
  paperBall.castShadow = true;
  paperGroup.add(paperBall);

  paperGroup.position.set(0.58, 1.55, 1.1);
  paperGroup.visible = false;
  scene.add(paperGroup);

  // ── Flat unfolded paper (stuck to wall) ───────────────────────────────────────
  const flatPaper = new Group();
  const sheetGeo = new PlaneGeometry(1.8, 1.2);
  const sheetMat = new MeshStandardMaterial({ color: 0xfffbeb, side: DoubleSide, roughness: 0.8 });
  const sheet = new Mesh(sheetGeo, sheetMat);
  flatPaper.add(sheet);

  // lines on paper (decoration)
  for (let l = 0; l < 6; l++) {
    const lineGeo = new PlaneGeometry(1.4, 0.02);
    const lineMat = new MeshStandardMaterial({ color: 0x94a3b8 });
    const line = new Mesh(lineGeo, lineMat);
    line.position.set(0, 0.42 - l * 0.14, 0.001);
    flatPaper.add(line);
  }

  flatPaper.position.set(0, 2.6, -7.9);
  flatPaper.visible = false;
  scene.add(flatPaper);

  // ── Animation state ───────────────────────────────────────────────────────────
  let phase: AnimPhase = 'idle';
  let t = 0; // phase-local timer [0..1]
  let pendingIdea = '';
  let frameId = 0;

  // Store home positions
  const paperHome = new Vector3(0.58, 1.55, 1.1);
  const wallTarget = new Vector3(0, 2.6, -7.9);

  // ── Easing ────────────────────────────────────────────────────────────────────
  const easeOut = (x: number) => 1 - (1 - x) ** 3;
  const easeInOut = (x: number) => x < 0.5 ? 2 * x * x : 1 - (-2 * x + 2) ** 2 / 2;

  // ── Render loop ───────────────────────────────────────────────────────────────
  let lastTime = 0;

  function animate(time: number) {
    frameId = requestAnimationFrame(animate);
    const dt = Math.min((time - lastTime) / 1000, 0.05);
    lastTime = time;

    // Idle breathing bob
    if (phase === 'idle') {
      potatoGroup.position.y = 1.28 + Math.sin(time * 0.002) * 0.025;
      throwArm.rotation.z = -Math.PI / 4 + Math.sin(time * 0.003) * 0.1;
    }

    // Wind-up
    if (phase === 'windup') {
      t = Math.min(t + dt * 2.5, 1);
      throwArm.rotation.z = MathUtils.lerp(-Math.PI / 4, Math.PI * 0.6, easeInOut(t));
      potatoGroup.rotation.z = MathUtils.lerp(0, -0.3, easeInOut(t));
      if (t >= 1) { phase = 'throw'; t = 0; }
    }

    // Throw
    if (phase === 'throw') {
      t = Math.min(t + dt * 4, 1);
      throwArm.rotation.z = MathUtils.lerp(Math.PI * 0.6, -Math.PI * 0.2, easeOut(t));
      potatoGroup.rotation.z = MathUtils.lerp(-0.3, 0.15, easeOut(t));
      if (t >= 0.4 && !paperGroup.visible) {
        paperGroup.visible = true;
      }
      if (t >= 1) { phase = 'flying'; t = 0; }
    }

    // Flying arc
    if (phase === 'flying') {
      t = Math.min(t + dt * 0.85, 1);
      const e = easeOut(t);
      paperGroup.position.lerpVectors(paperHome, wallTarget, e);
      // arc: rise then fall
      paperGroup.position.y += Math.sin(t * Math.PI) * 2.5;
      // spin
      paperGroup.rotation.x += dt * 8;
      paperGroup.rotation.y += dt * 5;
      if (t >= 1) {
        phase = 'stuck';
        t = 0;
        paperGroup.position.copy(wallTarget);
        paperGroup.rotation.set(0, 0, 0);
        // smush against wall
      }
    }

    // Stuck — brief pause + bounce
    if (phase === 'stuck') {
      t = Math.min(t + dt * 3, 1);
      const bounce = Math.abs(Math.sin(t * Math.PI * 2)) * (1 - t) * 0.3;
      paperBall.scale.setScalar(1 + bounce * 0.3);
      paperGroup.position.z = wallTarget.z + bounce * 0.3;
      if (t >= 1) {
        paperBall.scale.setScalar(1);
        paperGroup.position.copy(wallTarget);
        phase = 'unfolding';
        t = 0;
      }
    }

    // Unfolding — paper ball shrinks, flat paper grows
    if (phase === 'unfolding') {
      t = Math.min(t + dt * 1.2, 1);
      const e = easeInOut(t);
      paperBall.scale.setScalar(1 - e);
      flatPaper.visible = true;
      flatPaper.scale.setScalar(e);
      flatPaper.position.z = -7.9 + (1 - e) * 0.5;
      if (t >= 1) {
        paperGroup.visible = false;
        flatPaper.scale.setScalar(1);
        flatPaper.position.z = -7.9;
        phase = 'reading';
        t = 0;
        onReveal(pendingIdea);
        // reset arm after delay
        setTimeout(() => {
          throwArm.rotation.z = -Math.PI / 4;
          potatoGroup.rotation.z = 0;
        }, 600);
      }
    }

    // Reading — gentle paper shimmer
    if (phase === 'reading') {
      t += dt * 0.5;
      flatPaper.position.y = 2.6 + Math.sin(t) * 0.015;
    }

    renderer.render(scene, camera);
  }

  frameId = requestAnimationFrame(animate);

  // ── Resize observer ───────────────────────────────────────────────────────────
  const ro = new ResizeObserver(() => {
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  });
  ro.observe(canvas);

  // ── Public API ────────────────────────────────────────────────────────────────
  function trigger() {
    if (phase !== 'idle' && phase !== 'reading') return;
    pendingIdea = generateIdea();
    // reset paper
    flatPaper.visible = false;
    flatPaper.scale.setScalar(1);
    paperGroup.visible = false;
    paperGroup.position.copy(paperHome);
    paperGroup.rotation.set(0, 0, 0);
    paperBall.scale.setScalar(1);
    phase = 'windup';
    t = 0;
  }

  function dispose() {
    cancelAnimationFrame(frameId);
    ro.disconnect();
    renderer.dispose();
  }

  return { trigger, dispose };
}

// ── React component ───────────────────────────────────────────────────────────

interface Idea {
  id: number;
  text: string;
  copied: boolean;
}

export default function VibeIdeaGeneratorPage() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const sceneRef = useRef<SceneHandles | null>(null);
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [animating, setAnimating] = useState(false);
  const idCounter = useRef(0);

  useEffect(() => {
    if (!canvasRef.current) return;
    let mounted = true;
    buildScene(canvasRef.current, (idea) => {
      setIdeas((prev) => [
        { id: ++idCounter.current, text: idea, copied: false },
        ...prev,
      ]);
      setAnimating(false);
    }).then((handles) => {
      if (!mounted) { handles.dispose(); return; }
      sceneRef.current = handles;
    });
    return () => {
      mounted = false;
      sceneRef.current?.dispose();
      sceneRef.current = null;
    };
  }, []);

  const handleGenerate = useCallback(() => {
    if (animating) return;
    setAnimating(true);
    sceneRef.current?.trigger();
  }, [animating]);

  const copyToClipboard = useCallback((id: number, text: string) => {
    navigator.clipboard.writeText(text).then(() => {
      setIdeas((prev) =>
        prev.map((idea) => idea.id === id ? { ...idea, copied: true } : idea)
      );
      setTimeout(() => {
        setIdeas((prev) =>
          prev.map((idea) => idea.id === id ? { ...idea, copied: false } : idea)
        );
      }, 2000);
    });
  }, []);

  return (
    <Layout
      title="Vibe Idea Generator"
      description="Generate buzzword-packed vibe coding ideas by throwing paper at a wall."
    >
      <Head>
        <script type="application/ld+json">{JSON.stringify({
          '@context': 'https://schema.org',
          '@type': 'WebApplication',
          name: 'Vibe Idea Generator',
          description: 'Generate buzzword-packed vibe coding ideas by throwing paper at a wall.',
          url: 'https://treq.dev/tools/vibe-idea-generator',
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
            <span>Vibe Idea Generator</span>
          </div>
          <h1 className={styles.pageTitle}>Vibe Idea Generator ✨</h1>
          <p className={styles.pageSubtitle}>
            Throw ideas at the wall and see what sticks. Literally.
          </p>
        </div>

        <div className={styles.scene}>
          <canvas ref={canvasRef} className={styles.canvas} />
          <button
            className={styles.throwBtn}
            onClick={handleGenerate}
            disabled={animating}
          >
            {animating ? '🧻 throwing...' : '🥔 throw idea'}
          </button>
          {animating && (
            <div className={styles.hint}>Watch the potato go to work...</div>
          )}
        </div>

        {ideas.length > 0 && (
          <div className={styles.ideasSection}>
            <h2 className={styles.ideasTitle}>Ideas that stuck</h2>
            <ul className={styles.ideaList}>
              {ideas.map((idea) => (
                <li key={idea.id} className={styles.ideaItem}>
                  <span className={styles.ideaBullet}>💡</span>
                  <span className={styles.ideaText}>{idea.text}</span>
                  <button
                    className={styles.copyBtn}
                    onClick={() => copyToClipboard(idea.id, idea.text)}
                  >
                    {idea.copied ? '✓ copied' : 'copy'}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
      <DownloadCTA title="Built by Treq" />
    </Layout>
  );
}
