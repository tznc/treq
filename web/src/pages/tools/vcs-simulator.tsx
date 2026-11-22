import React, { useEffect, useMemo, useRef, useState } from 'react';
import Layout from '@theme/Layout';
import DownloadCTA from '@site/src/components/DownloadCTA';
import Head from '@docusaurus/Head';
import { renderGraph } from './_graph-shared';
import {
  SCENARIOS,
  type TerminalDef,
  type VcsTag,
  type SimStep,
  type Scenario,
} from './_vcs-simulator-scenarios';
import styles from './vcs-simulator.module.css';

// ── VCS badge ─────────────────────────────────────────────────────────────────

function VcsBadge({ vcs }: { vcs: VcsTag }) {
  const cls =
    vcs === 'git' ? styles.vcsBadgeGit : vcs === 'jj' ? styles.vcsBadgeJj : styles.vcsBadgeShell;
  return <span className={`${styles.vcsBadge} ${cls}`}>{vcs === 'shell' ? '$' : vcs}</span>;
}

// ── Terminal ───────────────────────────────────────────────────────────────────

function Terminal({
  step,
  def,
  isActive,
}: {
  step: SimStep | null;
  def?: TerminalDef;
  isActive?: boolean;
}) {
  const commandClass =
    step?.vcs === 'git'
      ? styles.terminalCommandGit
      : step?.vcs === 'jj'
        ? styles.terminalCommandJj
        : styles.terminalCommand;

  const prompt = def ? `${def.cwd} $ ` : '$ ';
  const inactive = def && isActive === false;

  return (
    <div className={`${styles.terminalPanel} ${inactive ? styles.terminalInactive : ''}`}>
      <div className={styles.terminalHeader}>
        <div className={styles.terminalDots}>
          <div className={styles.terminalDot} style={{ background: inactive ? '#475569' : '#ef4444' }} />
          <div className={styles.terminalDot} style={{ background: inactive ? '#475569' : '#f59e0b' }} />
          <div className={styles.terminalDot} style={{ background: inactive ? '#475569' : '#10b981' }} />
        </div>
        <span className={styles.terminalTitle}>
          {def ? def.label : 'terminal'}
        </span>
        {def && (
          <span className={styles.terminalCwd}>{def.cwd}</span>
        )}
        {def && isActive && (
          <span className={styles.terminalActivePill}>active</span>
        )}
      </div>
      <div className={`${styles.terminalBody} ${inactive ? styles.terminalBodyInactive : ''}`}>
        {!step ? (
          <span className={styles.terminalEmpty}>
            {def && !isActive ? 'Waiting...' : 'Select a step to execute it...'}
          </span>
        ) : (
          <>
            <div>
              <span className={styles.terminalPrompt}>{prompt}</span>
              <span className={commandClass}>{step.command}</span>
            </div>
            {step.output && <div className={styles.terminalOutput}>{step.output}</div>}
          </>
        )}
      </div>
    </div>
  );
}

// ── Multi-terminal layout ─────────────────────────────────────────────────────

function MultiTerminal({
  terminals,
  terminalStates,
  activeTerminalId,
}: {
  terminals: TerminalDef[];
  terminalStates: Record<string, SimStep | null>;
  activeTerminalId: string | undefined;
}) {
  return (
    <div className={styles.multiTerminal}>
      {terminals.map((term) => (
        <Terminal
          key={term.id}
          step={terminalStates[term.id] ?? null}
          def={term}
          isActive={term.id === activeTerminalId}
        />
      ))}
    </div>
  );
}

// ── Graph canvas ───────────────────────────────────────────────────────────────

function GraphCanvas({ step, dark }: { step: SimStep | null; dark: boolean }) {
  const svgRef = useRef<SVGSVGElement>(null);

  useEffect(() => {
    if (!svgRef.current) return;
    const state = step?.graphState ?? { branches: [], commits: [], title: '' };
    renderGraph(svgRef.current, state, dark, {
      highlightedId: step?.highlightedCommitId,
      headLabel: step?.headLabel,
    });
  }, [step, dark]);

  return (
    <div className={styles.graphPanel}>
      <div className={styles.graphHeader}>
        <span className={styles.graphTitle}>{step ? step.graphState.title : 'Commit graph'}</span>
        <span className={styles.graphMeta}>
          {step
            ? `${step.graphState.commits.length} commit${step.graphState.commits.length !== 1 ? 's' : ''}`
            : '—'}
        </span>
      </div>
      <div className={styles.graphCanvas}>
        <svg ref={svgRef} />
      </div>
    </div>
  );
}

// ── Terminal state helper ──────────────────────────────────────────────────────

function computeTerminalStates(
  scenario: Scenario,
  stepIdx: number,
): Record<string, SimStep | null> {
  if (!scenario.terminals) return {};
  const states: Record<string, SimStep | null> = {};
  for (const t of scenario.terminals) states[t.id] = null;
  for (const step of scenario.steps.slice(0, stepIdx + 1)) {
    const tid = step.terminalId ?? scenario.terminals[0]?.id;
    if (tid !== undefined && tid in states) states[tid] = step;
  }
  return states;
}

// ── Main page ─────────────────────────────────────────────────────────────────

export default function VcsSimulatorPage() {
  const [scenarioIdx, setScenarioIdx] = useState(0);
  const [stepIdx, setStepIdx] = useState(-1);
  const [dark, setDark] = useState(false);

  const scenario = SCENARIOS[scenarioIdx] ?? SCENARIOS[0];
  const currentStep = scenario.steps[stepIdx] ?? null;
  const totalSteps = scenario.steps.length;
  const isMultiTerminal = !!scenario.terminals;

  const terminalStates = useMemo(
    () => computeTerminalStates(scenario, stepIdx),
    [scenario, stepIdx],
  );

  const activeTerminalId = currentStep?.terminalId ?? scenario.terminals?.[0]?.id;

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    setDark(mq.matches);
    const handler = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, []);

  const handleScenario = (idx: number) => {
    setScenarioIdx(idx);
    setStepIdx(-1);
  };

  return (
    <Layout
      title="VCS Simulator"
      description="Step through git and jj commands interactively, watching the commit graph update in real time."
    >
      <Head>
        <script type="application/ld+json">{JSON.stringify({
          '@context': 'https://schema.org',
          '@type': 'WebApplication',
          name: 'VCS Simulator',
          description: 'Step through git and jj commands interactively, watching the commit graph update in real time.',
          url: 'https://treq.dev/tools/vcs-simulator',
          applicationCategory: 'DeveloperApplication',
          operatingSystem: 'Any',
          isAccessibleForFree: true,
          provider: {'@type': 'Organization', name: 'Treq', url: 'https://treq.dev'},
        })}</script>
      </Head>
      <div className={styles.page}>
        {/* Header */}
        <div className={styles.header}>
          <div className={styles.breadcrumb}>
            <a href="/tools">Tools</a>
            <span> / </span>
            <span>VCS Simulator</span>
          </div>
          <h1 className={styles.pageTitle}>VCS Simulator</h1>
          <p className={styles.pageSubtitle}>
            Step through git and jj commands interactively — watch the commit graph update in real
            time and see the terminal output for each command.
          </p>
        </div>

        {/* Scenario selector */}
        <div className={styles.scenarioTabs}>
          {SCENARIOS.map((s, i) => (
            <button
              key={s.id}
              className={`${styles.scenarioTab} ${i === scenarioIdx ? styles.scenarioTabActive : ''}`}
              onClick={() => handleScenario(i)}
            >
              <span className={styles.scenarioIcon}>{s.icon}</span>
              {s.name}
            </button>
          ))}
        </div>

        <p className={styles.scenarioDesc}>{scenario.description}</p>

        <div className={styles.scenarioTagRow}>
          {scenario.tags.map((t) => (
            <span key={t} className={styles.scenarioTag}>{t}</span>
          ))}
        </div>

        {/* Workspace */}
        <div className={styles.workspace}>
          {/* Left: steps panel */}
          <div className={styles.stepsPanel}>
            <div className={styles.stepsPanelHeader}>
              <span className={styles.stepsPanelTitle}>Steps</span>
              <span className={styles.stepCounter}>
                {stepIdx < 0 ? 0 : stepIdx + 1} / {totalSteps}
              </span>
            </div>

            <div className={styles.stepsList}>
              {scenario.steps.map((step, i) => {
                const isDone = i < stepIdx;
                const isActive = i === stepIdx;
                return (
                  <div
                    key={i}
                    className={`${styles.stepItem} ${isActive ? styles.stepItemActive : ''} ${isDone ? styles.stepItemDone : ''}`}
                    onClick={() => setStepIdx(i)}
                    role="button"
                    tabIndex={0}
                    onKeyDown={(e) => e.key === 'Enter' && setStepIdx(i)}
                    aria-label={`Step ${i + 1}: ${step.command}`}
                  >
                    <div
                      className={`${styles.stepStatus} ${isActive ? styles.stepStatusActive : isDone ? styles.stepStatusDone : styles.stepStatusPending}`}
                    >
                      {isDone ? '✓' : isActive ? '▶' : String(i + 1)}
                    </div>
                    <div className={styles.stepContent}>
                      <div className={styles.stepCommand}>
                        <VcsBadge vcs={step.vcs} />
                        {isMultiTerminal && step.terminalId && (
                          <span className={styles.stepTerminalBadge}>
                            {scenario.terminals!.find((t) => t.id === step.terminalId)?.label ?? step.terminalId}
                          </span>
                        )}
                        <span>{step.command}</span>
                      </div>
                      <div className={styles.stepDesc}>{step.description}</div>
                    </div>
                  </div>
                );
              })}
            </div>

            <div className={styles.stepsNav}>
              <button
                className={styles.navBtn}
                onClick={() => setStepIdx((i) => Math.max(-1, i - 1))}
                disabled={stepIdx < 0}
              >
                ← Back
              </button>
              <button
                className={`${styles.navBtn} ${styles.navBtnPrimary}`}
                onClick={() => setStepIdx((i) => Math.min(totalSteps - 1, i + 1))}
                disabled={stepIdx >= totalSteps - 1}
              >
                {stepIdx < 0 ? 'Start →' : 'Next →'}
              </button>
              <button
                className={styles.resetBtn}
                onClick={() => setStepIdx(-1)}
                disabled={stepIdx < 0}
                title="Reset to beginning"
              >
                ↺
              </button>
            </div>
          </div>

          {/* Right: graph + terminal(s) */}
          <div className={styles.rightPanel}>
            {currentStep && (
              <div className={styles.infoBox}>
                <span className={styles.infoBoxIcon}>💡</span>
                <span>{currentStep.description}</span>
              </div>
            )}

            <GraphCanvas step={currentStep} dark={dark} />

            {isMultiTerminal ? (
              <MultiTerminal
                terminals={scenario.terminals!}
                terminalStates={terminalStates}
                activeTerminalId={activeTerminalId}
              />
            ) : (
              <Terminal step={currentStep} />
            )}
          </div>
        </div>
      </div>
      <DownloadCTA title="Built by Treq" />
    </Layout>
  );
}
