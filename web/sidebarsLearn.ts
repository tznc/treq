import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

const sidebars: SidebarsConfig = {
  learnSidebar: [
    {
      type: 'doc',
      id: 'index',
      label: 'Overview',
    },
    {
      type: 'category',
      label: 'Concepts',
      link: {type: 'doc', id: 'concepts/index'},
      items: [
        {
          type: 'category',
          label: 'AI Engineering',
          link: {type: 'doc', id: 'concepts/ai-engineering/index'},
          items: [
            'concepts/ai-engineering/ai-assisted-software-engineering/index',
            'concepts/ai-engineering/human-in-the-loop-development',
            'concepts/ai-engineering/coding-agents',
            'concepts/ai-engineering/ai-code-review',
            'concepts/ai-engineering/human-in-the-loop-ai-code-review',
            'concepts/ai-engineering/parallel-coding-agents',
            'concepts/ai-engineering/agent-orchestration',
            'concepts/ai-engineering/quality-gates-for-agent-output',
          ],
        },
        {
          type: 'category',
          label: 'Git & Version Control',
          link: {type: 'doc', id: 'concepts/git/index'},
          items: [
            'concepts/git/version-control',
            'concepts/git/git-worktrees',
            'concepts/git/git-worktrees-for-ai',
            'concepts/git/git-worktrees-vs-clones',
            'concepts/git/git-submodules',
            'concepts/git/merge-vs-rebase',
            'concepts/git/git-reflog',
            'concepts/git/zdiff3',
            'concepts/git/git-rerere',
            'concepts/git/cherry-pick-vs-rebase',
            'concepts/git/force-with-lease',
            'concepts/git/stacked-prs',
            'concepts/git/stacked-prs-ai-assisted',
            'concepts/git/graphite-alternative',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: 'Workflows',
      link: {type: 'doc', id: 'workflows/index'},
      items: [
        {
          type: 'category',
          label: 'AI Workflows',
          link: {type: 'doc', id: 'workflows/ai/index'},
          items: [
            'workflows/ai/ai-feature-development',
            'workflows/ai/ai-bug-fix',
            'workflows/ai/ai-code-review',
            'workflows/ai/ai-refactoring',
            'workflows/ai/ai-documentation-writing',
            'workflows/ai/ai-marketing-copy',
          ],
        },
        {
          type: 'category',
          label: 'Git Workflows',
          link: {type: 'doc', id: 'workflows/git/index'},
          items: [
            'workflows/git/human-in-the-loop-review',
            'workflows/git/parallel-development',
            'workflows/git/stacked-pr',
          ],
        },
        {
          type: 'category',
          label: 'Frontend Workflows',
          link: {type: 'doc', id: 'workflows/frontend/index'},
          items: [
            'workflows/frontend/react-development',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: 'Tutorials',
      link: {type: 'doc', id: 'tutorials/index'},
      items: [
        'tutorials/setting-up-claude-code',
      ],
    },
    {
      type: 'category',
      label: 'How-To',
      link: {type: 'doc', id: 'how-to/index'},
      items: [
        'how-to/how-to-run-multiple-claude-code-sessions',
        'how-to/auto-rebase-ai-branches',
        'how-to/review-ai-generated-prs',
        'how-to/merge-conflicts-with-coding-agents',
        'how-to/git-worktrees-vs-clones-for-agents',
        'how-to/recover-deleted-git-branch',
      ],
    },
  ],
};

export default sidebars;
