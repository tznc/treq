import React, { useState, useCallback, useEffect, useRef } from 'react';
import styles from './gherkin-editor.module.css';

// ── Types ─────────────────────────────────────────────────────────────────────

type StepKeyword = 'Given' | 'When' | 'Then' | 'And' | 'But';

interface Step {
  id: string;
  keyword: StepKeyword;
  text: string;
}

interface Scenario {
  id: string;
  title: string;
  tags: string;
  steps: Step[];
}

interface Feature {
  id: string;
  title: string;
  description: string;
  tags: string;
  scenarios: Scenario[];
  updatedAt: number;
}

// ── Storage ───────────────────────────────────────────────────────────────────

const STORAGE_KEY = 'gherkin-editor-features-v1';

function loadFeatures(): Feature[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Feature[]) : [];
  } catch {
    return [];
  }
}

function saveFeatures(features: Feature[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(features));
}

// ── Factories ─────────────────────────────────────────────────────────────────

function uid() {
  return Math.random().toString(36).slice(2, 9);
}

function newStep(keyword: StepKeyword = 'Given'): Step {
  return { id: uid(), keyword, text: '' };
}

function newScenario(): Scenario {
  return {
    id: uid(),
    title: '',
    tags: '',
    steps: [newStep('Given'), newStep('When'), newStep('Then')],
  };
}

function newFeature(): Feature {
  return {
    id: uid(),
    title: '',
    description: '',
    tags: '',
    scenarios: [newScenario()],
    updatedAt: Date.now(),
  };
}

// ── Gherkin generator ─────────────────────────────────────────────────────────

function generateGherkin(feature: Feature): string {
  const lines: string[] = [];

  const featureTags = feature.tags
    .split(/[\s,]+/)
    .map((t) => t.trim())
    .filter(Boolean)
    .map((t) => (t.startsWith('@') ? t : `@${t}`))
    .join(' ');

  if (featureTags) lines.push(featureTags);

  lines.push(`Feature: ${feature.title || 'Untitled Feature'}`);

  if (feature.description.trim()) {
    feature.description.split('\n').forEach((line) => {
      lines.push(`  ${line}`);
    });
  }

  for (const scenario of feature.scenarios) {
    lines.push('');

    const scenarioTags = scenario.tags
      .split(/[\s,]+/)
      .map((t) => t.trim())
      .filter(Boolean)
      .map((t) => (t.startsWith('@') ? t : `@${t}`))
      .join(' ');

    if (scenarioTags) lines.push(`  ${scenarioTags}`);
    lines.push(`  Scenario: ${scenario.title || 'Untitled Scenario'}`);

    for (const step of scenario.steps) {
      if (step.text.trim()) {
        lines.push(`    ${step.keyword} ${step.text}`);
      }
    }
  }

  return lines.join('\n');
}

function formatDate(ts: number): string {
  return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}

// ── Toast ─────────────────────────────────────────────────────────────────────

function useToast() {
  const [msg, setMsg] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const show = useCallback((m: string) => {
    if (timer.current) clearTimeout(timer.current);
    setMsg(m);
    timer.current = setTimeout(() => setMsg(null), 2500);
  }, []);
  return { msg, show };
}

// ── Step row ──────────────────────────────────────────────────────────────────

interface StepRowProps {
  step: Step;
  isFirst: boolean;
  isLast: boolean;
  onChange: (patch: Partial<Step>) => void;
  onRemove: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}

const KEYWORDS: StepKeyword[] = ['Given', 'When', 'Then', 'And', 'But'];

function StepRow({ step, isFirst, isLast, onChange, onRemove, onMoveUp, onMoveDown }: StepRowProps) {
  return (
    <div className={styles.stepRow}>
      <div className={styles.stepReorder}>
        <button
          className={styles.reorderBtn}
          onClick={onMoveUp}
          disabled={isFirst}
          title="Move up"
          aria-label="Move step up"
        >
          ▲
        </button>
        <button
          className={styles.reorderBtn}
          onClick={onMoveDown}
          disabled={isLast}
          title="Move down"
          aria-label="Move step down"
        >
          ▼
        </button>
      </div>

      <select
        className={styles.keywordSelect}
        value={step.keyword}
        onChange={(e) => onChange({ keyword: e.target.value as StepKeyword })}
        aria-label="Step keyword"
      >
        {KEYWORDS.map((k) => (
          <option key={k} value={k}>{k}</option>
        ))}
      </select>

      <input
        className={styles.stepInput}
        value={step.text}
        placeholder="step description…"
        onChange={(e) => onChange({ text: e.target.value })}
        aria-label="Step text"
      />

      <button
        className={styles.removeStepBtn}
        onClick={onRemove}
        title="Remove step"
        aria-label="Remove step"
      >
        ✕
      </button>
    </div>
  );
}

// ── Scenario block ────────────────────────────────────────────────────────────

interface ScenarioBlockProps {
  scenario: Scenario;
  index: number;
  isFirst: boolean;
  isLast: boolean;
  onChange: (patch: Partial<Scenario>) => void;
  onRemove: () => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
}

function ScenarioBlock({ scenario, index, isFirst, isLast, onChange, onRemove, onMoveUp, onMoveDown }: ScenarioBlockProps) {
  const [collapsed, setCollapsed] = useState(false);

  const updateStep = (stepId: string, patch: Partial<Step>) => {
    onChange({
      steps: scenario.steps.map((s) => (s.id === stepId ? { ...s, ...patch } : s)),
    });
  };

  const removeStep = (stepId: string) => {
    onChange({ steps: scenario.steps.filter((s) => s.id !== stepId) });
  };

  const addStep = () => {
    const lastKeyword = scenario.steps.at(-1)?.keyword ?? 'Then';
    const nextKeyword: StepKeyword = lastKeyword === 'Given' ? 'When' : lastKeyword === 'When' ? 'Then' : 'And';
    onChange({ steps: [...scenario.steps, newStep(nextKeyword)] });
  };

  const moveStep = (idx: number, dir: -1 | 1) => {
    const steps = [...scenario.steps];
    const target = idx + dir;
    if (target < 0 || target >= steps.length) return;
    steps.splice(target, 0, ...steps.splice(idx, 1));
    onChange({ steps });
  };

  return (
    <div className={styles.scenarioBlock}>
      <div className={styles.scenarioHeader}>
        <div className={styles.scenarioMeta}>
          <button
            className={styles.collapseBtn}
            onClick={() => setCollapsed((c) => !c)}
            aria-label={collapsed ? 'Expand scenario' : 'Collapse scenario'}
          >
            {collapsed ? '▶' : '▼'}
          </button>
          <span className={styles.scenarioLabel}>Scenario {index + 1}</span>
        </div>
        <div className={styles.scenarioActions}>
          <button className={styles.scenarioMoveBtn} onClick={onMoveUp} disabled={isFirst} title="Move up" aria-label="Move scenario up">↑</button>
          <button className={styles.scenarioMoveBtn} onClick={onMoveDown} disabled={isLast} title="Move down" aria-label="Move scenario down">↓</button>
          <button className={styles.removeScenarioBtn} onClick={onRemove} title="Remove scenario" aria-label="Remove scenario">✕</button>
        </div>
      </div>

      {!collapsed && (
        <div className={styles.scenarioBody}>
          <div className={styles.fieldGroup}>
            <label className={styles.fieldLabel}>Title</label>
            <input
              className={styles.fieldInput}
              value={scenario.title}
              placeholder="e.g. Successful login with valid credentials"
              onChange={(e) => onChange({ title: e.target.value })}
            />
          </div>

          <div className={styles.fieldGroup}>
            <label className={styles.fieldLabel}>Tags <span className={styles.optional}>(space or comma separated)</span></label>
            <input
              className={styles.fieldInput}
              value={scenario.tags}
              placeholder="e.g. @smoke @regression"
              onChange={(e) => onChange({ tags: e.target.value })}
            />
          </div>

          <div className={styles.stepsSection}>
            <div className={styles.stepsSectionHeader}>
              <span className={styles.fieldLabel}>Steps</span>
            </div>

            {scenario.steps.length === 0 && (
              <div className={styles.emptySteps}>No steps yet.</div>
            )}

            {scenario.steps.map((step, si) => (
              <StepRow
                key={step.id}
                step={step}
                isFirst={si === 0}
                isLast={si === scenario.steps.length - 1}
                onChange={(patch) => updateStep(step.id, patch)}
                onRemove={() => removeStep(step.id)}
                onMoveUp={() => moveStep(si, -1)}
                onMoveDown={() => moveStep(si, 1)}
              />
            ))}

            <button className={styles.addStepBtn} onClick={addStep}>
              + Add step
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Feature editor ────────────────────────────────────────────────────────────

interface FeatureEditorProps {
  feature: Feature;
  onChange: (patch: Partial<Feature>) => void;
  onDelete: () => void;
}

function FeatureEditor({ feature, onChange, onDelete }: FeatureEditorProps) {
  const updateScenario = (scenarioId: string, patch: Partial<Scenario>) => {
    onChange({
      scenarios: feature.scenarios.map((s) =>
        s.id === scenarioId ? { ...s, ...patch } : s
      ),
    });
  };

  const removeScenario = (scenarioId: string) => {
    onChange({ scenarios: feature.scenarios.filter((s) => s.id !== scenarioId) });
  };

  const addScenario = () => {
    onChange({ scenarios: [...feature.scenarios, newScenario()] });
  };

  const moveScenario = (idx: number, dir: -1 | 1) => {
    const scenarios = [...feature.scenarios];
    const target = idx + dir;
    if (target < 0 || target >= scenarios.length) return;
    scenarios.splice(target, 0, ...scenarios.splice(idx, 1));
    onChange({ scenarios });
  };

  return (
    <div className={styles.editor}>
      <div className={styles.editorSection}>
        <div className={styles.sectionHeader}>
          <h2 className={styles.sectionTitle}>Feature</h2>
          <button className={styles.deleteFeatBtn} onClick={onDelete}>Delete feature</button>
        </div>

        <div className={styles.fieldGroup}>
          <label className={styles.fieldLabel}>Title</label>
          <input
            className={styles.fieldInput}
            value={feature.title}
            placeholder="e.g. User Authentication"
            onChange={(e) => onChange({ title: e.target.value })}
          />
        </div>

        <div className={styles.fieldGroup}>
          <label className={styles.fieldLabel}>Description <span className={styles.optional}>(optional)</span></label>
          <textarea
            className={styles.fieldTextarea}
            rows={3}
            value={feature.description}
            placeholder="As a user, I want to log in so that I can access my account."
            onChange={(e) => onChange({ description: e.target.value })}
          />
        </div>

        <div className={styles.fieldGroup}>
          <label className={styles.fieldLabel}>Tags <span className={styles.optional}>(space or comma separated)</span></label>
          <input
            className={styles.fieldInput}
            value={feature.tags}
            placeholder="e.g. @auth @critical"
            onChange={(e) => onChange({ tags: e.target.value })}
          />
        </div>
      </div>

      <div className={styles.editorSection}>
        <div className={styles.sectionHeader}>
          <h2 className={styles.sectionTitle}>Scenarios</h2>
          <button className={styles.addScenarioBtn} onClick={addScenario}>+ Add scenario</button>
        </div>

        {feature.scenarios.length === 0 && (
          <div className={styles.emptyScenarios}>No scenarios yet. Add one above.</div>
        )}

        {feature.scenarios.map((scenario, idx) => (
          <ScenarioBlock
            key={scenario.id}
            scenario={scenario}
            index={idx}
            isFirst={idx === 0}
            isLast={idx === feature.scenarios.length - 1}
            onChange={(patch) => updateScenario(scenario.id, patch)}
            onRemove={() => removeScenario(scenario.id)}
            onMoveUp={() => moveScenario(idx, -1)}
            onMoveDown={() => moveScenario(idx, 1)}
          />
        ))}
      </div>
    </div>
  );
}

// ── Main content ──────────────────────────────────────────────────────────────

export function GherkinEditorContent() {
  const [features, setFeatures] = useState<Feature[]>(loadFeatures);
  const [activeId, setActiveId] = useState<string | null>(
    () => loadFeatures()[0]?.id ?? null
  );
  const { msg: toast, show: showToast } = useToast();

  const active = features.find((f) => f.id === activeId) ?? null;

  useEffect(() => {
    saveFeatures(features);
  }, [features]);

  const createFeature = useCallback(() => {
    const f = newFeature();
    setFeatures((prev) => [f, ...prev]);
    setActiveId(f.id);
  }, []);

  const updateActive = useCallback((patch: Partial<Feature>) => {
    if (!activeId) return;
    setFeatures((prev) =>
      prev.map((f) =>
        f.id === activeId ? { ...f, ...patch, updatedAt: Date.now() } : f
      )
    );
  }, [activeId]);

  const deleteFeature = useCallback((id: string) => {
    setFeatures((prev) => {
      const next = prev.filter((f) => f.id !== id);
      if (activeId === id) setActiveId(next[0]?.id ?? null);
      return next;
    });
  }, [activeId]);

  const copyGherkin = useCallback(() => {
    if (!active) return;
    navigator.clipboard.writeText(generateGherkin(active)).then(() => {
      showToast('Gherkin copied to clipboard!');
    });
  }, [active, showToast]);

  const downloadFeature = useCallback(() => {
    if (!active) return;
    const text = generateGherkin(active);
    const slug = (active.title || 'untitled').toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '');
    const blob = new Blob([text], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `${slug}.feature`;
    a.click();
  }, [active]);

  return (
    <div className={styles.page}>
      {toast && <div className={styles.toast}>{toast}</div>}

      <div className={styles.pageHeader}>
        <div className={styles.breadcrumb}>
          <a href="/tools">Tools</a>
          <span> / </span>
          <span>Gherkin BDD Editor</span>
        </div>
        <h1 className={styles.pageTitle}>Gherkin BDD Editor</h1>
        <p className={styles.pageSubtitle}>
          Write BDD specs in a structured form and export to <code>.feature</code> files.
          Features are saved in your browser&apos;s local storage.
        </p>
      </div>

      <div className={styles.workspace}>
        {/* Sidebar: feature library */}
        <div className={styles.sidebar}>
          <div className={styles.sidebarHeader}>
            <span className={styles.sidebarTitle}>Features</span>
            <button className={styles.newBtn} onClick={createFeature}>+ New</button>
          </div>

          {features.length === 0 ? (
            <div className={styles.sidebarEmpty}>
              No features yet.
              <br />
              Create one to start.
            </div>
          ) : (
            <div className={styles.featureList}>
              {features.map((f) => (
                <button
                  key={f.id}
                  className={`${styles.featureItem} ${f.id === activeId ? styles.featureItemActive : ''}`}
                  onClick={() => setActiveId(f.id)}
                >
                  <span className={styles.featureItemTitle}>{f.title || 'Untitled'}</span>
                  <span className={styles.featureItemMeta}>
                    {f.scenarios.length} scenario{f.scenarios.length !== 1 ? 's' : ''} · {formatDate(f.updatedAt)}
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Center: form editor */}
        {active ? (
          <FeatureEditor
            feature={active}
            onChange={updateActive}
            onDelete={() => deleteFeature(active.id)}
          />
        ) : (
          <div className={styles.emptyState}>
            <div className={styles.emptyStateIcon}>📋</div>
            <p className={styles.emptyStateText}>
              Select a feature from the sidebar or create a new one.
            </p>
            <button className={styles.emptyStateBtn} onClick={createFeature}>
              Create Feature
            </button>
          </div>
        )}

        {/* Right: preview */}
        {active && (
          <div className={styles.preview}>
            <div className={styles.previewHeader}>
              <span className={styles.previewTitle}>.feature preview</span>
              <div className={styles.previewActions}>
                <button className={styles.actionBtn} onClick={copyGherkin} title="Copy to clipboard">
                  Copy
                </button>
                <button className={styles.actionBtn} onClick={downloadFeature} title="Download .feature file">
                  ↓ Download
                </button>
              </div>
            </div>
            <pre className={styles.previewCode}>{generateGherkin(active)}</pre>
          </div>
        )}
      </div>
    </div>
  );
}
