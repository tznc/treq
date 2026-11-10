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
  ],
};

export default sidebars;
