import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  compareSidebar: [
    {
      type: 'doc',
      id: 'index',
      label: 'Overview',
    },
    'treq-vs-superset',
    'treq-vs-conductor',
    'treq-vs-emdash',
    'treq-vs-claude-squad',
    'treq-vs-graphite-for-ai-teams',
    'treq-vs-orca',
    'treq-vs-t3-code',
    'treq-vs-vibe-kanban',
    'treq-vs-github-stacked-prs',
    'treq-vs-gitbutler',
    'treq-vs-herdr',
    {
      type: 'category',
      label: 'Best Tools',
      items: [
        'best-parallel-agent-tools',
        'best-graphite-alternatives',
        'best-git-worktree-managers',
        'sandbox-providers-for-coding-agents',
      ],
    },
    {
      type: 'category',
      label: 'Head-to-Head',
      items: [
        'orca-vs-superset',
        'conductor-vs-superset',
        'vibe-kanban-vs-claude-squad',
        'graphite-vs-gitbutler',
        'jj-vs-git',
      ],
    },
    {
      type: 'category',
      label: 'Models',
      items: [
        'best-llms-for-coding-agents',
        'claude-vs-gpt-for-coding',
        'claude-vs-gemini-for-coding',
        'best-open-weight-coding-models',
      ],
    },
    {
      type: 'category',
      label: 'Coding Agents',
      items: [
        'best-coding-agent-clis',
        'claude-code-vs-codex-cli',
        'claude-code-vs-gemini-cli',
        'opencode-vs-claude-code',
        'cursor-vs-claude-code',
      ],
    },
  ],
};

export default sidebars;
