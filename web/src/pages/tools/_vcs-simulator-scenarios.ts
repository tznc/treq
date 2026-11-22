import type { GraphState } from './_graph-shared';

// ── Types ─────────────────────────────────────────────────────────────────────

export type VcsTag = 'git' | 'jj' | 'shell';

export interface TerminalDef {
  id: string;
  label: string;
  cwd: string;
}

export interface SimStep {
  command: string;
  vcs: VcsTag;
  description: string;
  output: string;
  graphState: GraphState;
  highlightedCommitId?: string;
  headLabel?: string;
  terminalId?: string;
}

export interface Scenario {
  id: string;
  name: string;
  description: string;
  icon: string;
  tags: string[];
  terminals?: TerminalDef[];
  steps: SimStep[];
}

// ── Shared branch defs ────────────────────────────────────────────────────────

const MAIN = { name: 'main', color: '#6366f1' };
const FEAT = { name: 'feature', color: '#f59e0b' };
const FEAT_LOGIN = { name: 'feature/login', color: '#f59e0b' };
const JJ_MAIN = { name: 'main', color: '#6366f1' };
const JJ_FORK = { name: 'fork', color: '#ef4444' };
const FEAT_SEARCH = { name: 'feature/search', color: '#10b981' };

// ── Scenario 1: Git: Branch Merge ────────────────────────────────────────────

const GIT_BRANCH_MERGE: Scenario = {
  id: 'git-branch-merge',
  name: 'Git: Branch Merge',
  description:
    'The classic Git integration strategy: develop on a feature branch and bring work back to main with an explicit merge commit that preserves the full fork in history.',
  icon: '🌿',
  tags: ['git', 'branching', 'merge'],
  steps: [
    {
      command: 'git init my-app && cd my-app',
      vcs: 'shell',
      description: 'Initialize an empty Git repository in a new directory.',
      output: "hint: Using 'main' as the initial branch name.\nInitialized empty Git repository in /projects/my-app/.git/",
      graphState: { title: 'Empty repository', branches: [MAIN], commits: [] },
    },
    {
      command: 'git commit --allow-empty -m "Initial commit"',
      vcs: 'git',
      description: 'Create the first commit on main — the root of the repository.',
      output: '[main (root-commit) a1b2c3d] Initial commit',
      graphState: {
        title: 'First commit on main',
        branches: [MAIN],
        commits: [{ id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] }],
      },
      highlightedCommitId: 'a1b2c3d',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout -b feature/login',
      vcs: 'git',
      description: 'Create and switch to a new feature branch. Both main and feature/login point to the same commit for now.',
      output: "Switched to a new branch 'feature/login'",
      graphState: {
        title: 'New branch: feature/login',
        branches: [MAIN, FEAT_LOGIN],
        commits: [{ id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main', 'feature/login'] }],
      },
      highlightedCommitId: 'a1b2c3d',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit -m "Add login form"',
      vcs: 'git',
      description: 'Commit on the feature branch. main stays behind while feature/login advances.',
      output: '[feature/login d4e5f6g] Add login form\n 1 file changed, 42 insertions(+)',
      graphState: {
        title: 'First commit on feature/login',
        branches: [MAIN, FEAT_LOGIN],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'd4e5f6g', label: 'Add login form', branch: 'feature/login', parents: ['a1b2c3d'], bookmarks: ['feature/login'] },
        ],
      },
      highlightedCommitId: 'd4e5f6g',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit -m "Add auth middleware"',
      vcs: 'git',
      description: 'A second commit on the feature branch.',
      output: '[feature/login h7i8j9k] Add auth middleware\n 2 files changed, 78 insertions(+)',
      graphState: {
        title: 'Two commits on feature/login',
        branches: [MAIN, FEAT_LOGIN],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'd4e5f6g', label: 'Add login form', branch: 'feature/login', parents: ['a1b2c3d'] },
          { id: 'h7i8j9k', label: 'Add auth middleware', branch: 'feature/login', parents: ['d4e5f6g'], bookmarks: ['feature/login'] },
        ],
      },
      highlightedCommitId: 'h7i8j9k',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout main && git commit -m "Update dependencies"',
      vcs: 'git',
      description: 'Switch back to main and add a commit. Now main and feature/login have diverged — each has commits the other lacks.',
      output: "Switched to branch 'main'\n[main l0m1n2o] Update dependencies\n 1 file changed, 5 insertions(+)",
      graphState: {
        title: 'Diverged: both branches have unique commits',
        branches: [MAIN, FEAT_LOGIN],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'd4e5f6g', label: 'Add login form', branch: 'feature/login', parents: ['a1b2c3d'] },
          { id: 'h7i8j9k', label: 'Add auth middleware', branch: 'feature/login', parents: ['d4e5f6g'], bookmarks: ['feature/login'] },
          { id: 'l0m1n2o', label: 'Update dependencies', branch: 'main', parents: ['a1b2c3d'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'l0m1n2o',
      headLabel: 'HEAD',
    },
    {
      command: 'git merge feature/login',
      vcs: 'git',
      description: 'Because the branches diverged, Git creates a merge commit with two parents — one from each branch — preserving the full history fork.',
      output: "Merge made by the 'ort' strategy.\n src/auth.js  | 78 ++++++\n src/login.jsx | 42 ++++++\n 2 files changed, 120 insertions(+)",
      graphState: {
        title: 'Merged with a two-parent merge commit',
        branches: [MAIN, FEAT_LOGIN],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'd4e5f6g', label: 'Add login form', branch: 'feature/login', parents: ['a1b2c3d'] },
          { id: 'h7i8j9k', label: 'Add auth middleware', branch: 'feature/login', parents: ['d4e5f6g'], bookmarks: ['feature/login'] },
          { id: 'l0m1n2o', label: 'Update dependencies', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'p3q4r5s', label: "Merge 'feature/login'", branch: 'main', parents: ['l0m1n2o', 'h7i8j9k'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'p3q4r5s',
      headLabel: 'HEAD',
    },
    {
      command: 'git branch -d feature/login',
      vcs: 'git',
      description: 'Delete the branch label. The commits remain reachable through the merge commit — only the pointer is removed.',
      output: 'Deleted branch feature/login (was h7i8j9k).',
      graphState: {
        title: 'Branch label removed; history intact',
        branches: [MAIN, FEAT_LOGIN],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'd4e5f6g', label: 'Add login form', branch: 'feature/login', parents: ['a1b2c3d'] },
          { id: 'h7i8j9k', label: 'Add auth middleware', branch: 'feature/login', parents: ['d4e5f6g'] },
          { id: 'l0m1n2o', label: 'Update dependencies', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'p3q4r5s', label: "Merge 'feature/login'", branch: 'main', parents: ['l0m1n2o', 'h7i8j9k'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'p3q4r5s',
      headLabel: 'HEAD',
    },
  ],
};

// ── Scenario 2: Git: Branch Rebase ───────────────────────────────────────────

const GIT_BRANCH_REBASE: Scenario = {
  id: 'git-branch-rebase',
  name: 'Git: Branch Rebase',
  description:
    'Rebase replays your feature commits on top of the latest main, producing a linear history with no merge commit. Git rewrites commit IDs in the process.',
  icon: '🔀',
  tags: ['git', 'rebase', 'linear-history'],
  steps: [
    {
      command: 'git init && git commit --allow-empty -m "Initial commit"',
      vcs: 'git',
      description: 'Create a repository with one commit on main.',
      output: '[main (root-commit) a1b2c3d] Initial commit',
      graphState: {
        title: 'Starting point',
        branches: [MAIN],
        commits: [{ id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] }],
      },
      highlightedCommitId: 'a1b2c3d',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout -b feature && git commit -m "Feature: add search"',
      vcs: 'git',
      description: 'Branch off main and make the first feature commit.',
      output: "Switched to a new branch 'feature'\n[feature b3c4d5e] Feature: add search\n 1 file changed, 20 insertions(+)",
      graphState: {
        title: 'Feature branch: one commit',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b3c4d5e', label: 'Feature: add search', branch: 'feature', parents: ['a1b2c3d'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'b3c4d5e',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit -m "Feature: add filters"',
      vcs: 'git',
      description: 'A second feature commit. The feature branch now has two commits, both based on the original main.',
      output: '[feature c5d6e7f] Feature: add filters\n 1 file changed, 15 insertions(+)',
      graphState: {
        title: 'Feature: two commits',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b3c4d5e', label: 'Feature: add search', branch: 'feature', parents: ['a1b2c3d'] },
          { id: 'c5d6e7f', label: 'Feature: add filters', branch: 'feature', parents: ['b3c4d5e'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'c5d6e7f',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout main && git commit -m "Hotfix: patch auth bug"',
      vcs: 'git',
      description: 'Meanwhile, main receives a hotfix. The branches have now diverged — this is why a plain fast-forward won\'t work.',
      output: "Switched to branch 'main'\n[main d7e8f9a] Hotfix: patch auth bug\n 1 file changed, 3 insertions(+), 2 deletions(-)",
      graphState: {
        title: 'Diverged: main got a hotfix',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'b3c4d5e', label: 'Feature: add search', branch: 'feature', parents: ['a1b2c3d'] },
          { id: 'c5d6e7f', label: 'Feature: add filters', branch: 'feature', parents: ['b3c4d5e'], bookmarks: ['feature'] },
          { id: 'd7e8f9a', label: 'Hotfix: patch auth bug', branch: 'main', parents: ['a1b2c3d'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'd7e8f9a',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout feature && git rebase main',
      vcs: 'git',
      description: 'Rebase feature onto main. Git detaches the feature commits from their old base, then replays them one-by-one on top of main\'s hotfix. New commit IDs are assigned.',
      output: "Switched to branch 'feature'\nSuccessfully rebased and updated refs/heads/feature.",
      graphState: {
        title: 'After rebase: feature sits atop the hotfix',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'd7e8f9a', label: 'Hotfix: patch auth bug', branch: 'main', parents: ['a1b2c3d'], bookmarks: ['main'] },
          { id: 'e9b3c4d', label: "Feature: add search (rebased)", branch: 'feature', parents: ['d7e8f9a'] },
          { id: 'f0c5d6e', label: "Feature: add filters (rebased)", branch: 'feature', parents: ['e9b3c4d'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'f0c5d6e',
      headLabel: 'HEAD',
    },
    {
      command: 'git log --oneline',
      vcs: 'git',
      description: 'Verify the linear history. Notice the commit IDs changed after rebase — git literally wrote new commits. The old b3c4d5e and c5d6e7f no longer exist on this branch.',
      output: 'f0c5d6e (HEAD -> feature) Feature: add filters (rebased)\ne9b3c4d Feature: add search (rebased)\nd7e8f9a (main) Hotfix: patch auth bug\na1b2c3d Initial commit',
      graphState: {
        title: 'Linear: new commit IDs after rebase',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'd7e8f9a', label: 'Hotfix: patch auth bug', branch: 'main', parents: ['a1b2c3d'], bookmarks: ['main'] },
          { id: 'e9b3c4d', label: "Feature: add search (rebased)", branch: 'feature', parents: ['d7e8f9a'] },
          { id: 'f0c5d6e', label: "Feature: add filters (rebased)", branch: 'feature', parents: ['e9b3c4d'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'f0c5d6e',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout main && git merge feature',
      vcs: 'git',
      description: 'Because feature is now directly ahead of main, the merge is a fast-forward: main simply moves its pointer forward. No merge commit is created.',
      output: "Switched to branch 'main'\nUpdating d7e8f9a..f0c5d6e\nFast-forward\n 2 files changed, 35 insertions(+)",
      graphState: {
        title: 'Fast-forward merge: perfectly linear history',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'd7e8f9a', label: 'Hotfix: patch auth bug', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'e9b3c4d', label: 'Feature: add search', branch: 'feature', parents: ['d7e8f9a'] },
          { id: 'f0c5d6e', label: 'Feature: add filters', branch: 'feature', parents: ['e9b3c4d'], bookmarks: ['main', 'feature'] },
        ],
      },
      highlightedCommitId: 'f0c5d6e',
      headLabel: 'HEAD',
    },
  ],
};

// ── Scenario 3: Jujitsu: Stacked Changes ─────────────────────────────────────

const JJ_STACKED: Scenario = {
  id: 'jj-stacked',
  name: 'Jujitsu: Stacked Changes',
  description:
    'jj treats every change as a first-class object. Build a stack of 4+ in-progress changes, then insert a fix mid-stack without losing context.',
  icon: '🥞',
  tags: ['jj', 'stacking', 'rebase', 'squash'],
  steps: [
    {
      command: 'jj git init --colocate',
      vcs: 'jj',
      description: 'Initialize a jj repo backed by git. The --colocate flag places .jj and .git in the same directory so both tools work on the same data.',
      output: 'Done. The colocated Git repo has been initialized.\nWorking copy now at: zzzzzzzz (empty)\nParent commit      : 000000000000 (root)',
      graphState: {
        title: 'Fresh jj repo',
        branches: [JJ_MAIN],
        commits: [{ id: 'zzzzzzzz', label: '(empty working copy)', branch: 'main', parents: [], isWorkingCopy: true }],
      },
      highlightedCommitId: 'zzzzzzzz',
      headLabel: '@',
    },
    {
      command: 'jj new -m "Scaffold project"',
      vcs: 'jj',
      description: 'Create the first change. jj moves @ to the new change automatically — no separate staging or commit step needed.',
      output: 'Working copy now at: qpvuntsm Scaffold project\nParent commit      : zzzzzzzz (root)',
      graphState: {
        title: 'Stack: 1 change',
        branches: [JJ_MAIN],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'qpvuntsm',
      headLabel: '@',
    },
    {
      command: 'jj new -m "Add database models"',
      vcs: 'jj',
      description: 'Stack a second change on top. In jj, you think of these as "changes in progress" rather than "commits to squash later".',
      output: 'Working copy now at: rlvkpnvx Add database models\nParent commit      : qpvuntsm Scaffold project',
      graphState: {
        title: 'Stack: 2 changes',
        branches: [JJ_MAIN],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'] },
          { id: 'rlvkpnvx', label: 'Add database models', branch: 'main', parents: ['qpvuntsm'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'rlvkpnvx',
      headLabel: '@',
    },
    {
      command: 'jj new -m "Add API routes"',
      vcs: 'jj',
      description: 'A third change, building on the database models.',
      output: 'Working copy now at: mzvwutvl Add API routes\nParent commit      : rlvkpnvx Add database models',
      graphState: {
        title: 'Stack: 3 changes',
        branches: [JJ_MAIN],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'] },
          { id: 'rlvkpnvx', label: 'Add database models', branch: 'main', parents: ['qpvuntsm'] },
          { id: 'mzvwutvl', label: 'Add API routes', branch: 'main', parents: ['rlvkpnvx'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'mzvwutvl',
      headLabel: '@',
    },
    {
      command: 'jj new -m "Add frontend views"',
      vcs: 'jj',
      description: 'A fourth change. Now we have 4 stacked in-progress changes — each depending on the one below.',
      output: 'Working copy now at: yostqsyk Add frontend views\nParent commit      : mzvwutvl Add API routes',
      graphState: {
        title: 'Stack: 4 changes deep',
        branches: [JJ_MAIN],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'] },
          { id: 'rlvkpnvx', label: 'Add database models', branch: 'main', parents: ['qpvuntsm'] },
          { id: 'mzvwutvl', label: 'Add API routes', branch: 'main', parents: ['rlvkpnvx'] },
          { id: 'yostqsyk', label: 'Add frontend views', branch: 'main', parents: ['mzvwutvl'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'yostqsyk',
      headLabel: '@',
    },
    {
      command: 'jj new @-- -m "Fix: DB connection pool limit"',
      vcs: 'jj',
      description: '@-- means "grandparent of @" — two steps up from frontend views is Add database models. jj creates a new change there, forking the stack without disturbing the others.',
      output: 'Working copy now at: kxvzptqn Fix: DB connection pool limit\nParent commit      : rlvkpnvx Add database models',
      graphState: {
        title: 'Fork: inserted fix mid-stack',
        branches: [JJ_MAIN, JJ_FORK],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'] },
          { id: 'rlvkpnvx', label: 'Add database models', branch: 'main', parents: ['qpvuntsm'] },
          { id: 'mzvwutvl', label: 'Add API routes', branch: 'main', parents: ['rlvkpnvx'] },
          { id: 'yostqsyk', label: 'Add frontend views', branch: 'main', parents: ['mzvwutvl'] },
          { id: 'kxvzptqn', label: 'Fix: DB connection pool limit', branch: 'fork', parents: ['rlvkpnvx'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'kxvzptqn',
      headLabel: '@',
    },
    {
      command: 'jj rebase -s mzvwutvl -d @',
      vcs: 'jj',
      description: 'Rebase the "Add API routes" change (and all its descendants: frontend views) onto the current @ (the fix). jj moves the entire subtree atomically.',
      output: 'Rebased 2 commits onto destination\nWorking copy now at: kxvzptqn Fix: DB connection pool limit',
      graphState: {
        title: 'Stack rebased: fix slotted in',
        branches: [JJ_MAIN, JJ_FORK],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'] },
          { id: 'rlvkpnvx', label: 'Add database models', branch: 'main', parents: ['qpvuntsm'] },
          { id: 'kxvzptqn', label: 'Fix: DB connection pool limit', branch: 'fork', parents: ['rlvkpnvx'], isWorkingCopy: true },
          { id: 'mzvwutvl', label: 'Add API routes', branch: 'main', parents: ['kxvzptqn'] },
          { id: 'yostqsyk', label: 'Add frontend views', branch: 'main', parents: ['mzvwutvl'] },
        ],
      },
      highlightedCommitId: 'kxvzptqn',
      headLabel: '@',
    },
    {
      command: 'jj squash',
      vcs: 'jj',
      description: 'Squash @ (the fix) into its parent (Add database models). jj merges their content and removes the fix commit — the stack is linear again with 4 changes.',
      output: 'Working copy now at: mzvwutvl Add API routes\nParent commit      : rlvkpnvx Add database models (squashed fix)',
      graphState: {
        title: 'Fix absorbed — linear 4-change stack',
        branches: [JJ_MAIN],
        commits: [
          { id: 'zzzzzzzz', label: '(root)', branch: 'main', parents: [] },
          { id: 'qpvuntsm', label: 'Scaffold project', branch: 'main', parents: ['zzzzzzzz'] },
          { id: 'rlvkpnvx', label: 'Add database models (+fix)', branch: 'main', parents: ['qpvuntsm'], bookmarks: ['main'] },
          { id: 'mzvwutvl', label: 'Add API routes', branch: 'main', parents: ['rlvkpnvx'] },
          { id: 'yostqsyk', label: 'Add frontend views', branch: 'main', parents: ['mzvwutvl'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'yostqsyk',
      headLabel: '@',
    },
  ],
};

// ── Scenario 4: jj ↔ git Interop (switching tools) ───────────────────────────

const JJ_GIT_INTEROP: Scenario = {
  id: 'jj-git-interop',
  name: 'jj ↔ git Interop',
  description:
    'jj stores everything in git\'s object model. Use git and jj commands interchangeably on the same repo — each tool sees every commit the other makes in real time.',
  icon: '🔄',
  tags: ['jj', 'git', 'interop', 'colocate'],
  steps: [
    {
      command: 'git init my-app && cd my-app',
      vcs: 'shell',
      description: 'Start with a plain git repository. No jj yet.',
      output: "Initialized empty Git repository in /projects/my-app/.git/",
      graphState: { title: 'Plain git repo', branches: [MAIN], commits: [] },
    },
    {
      command: 'git commit --allow-empty -m "Initial commit"',
      vcs: 'git',
      description: 'Create the first commit using plain git.',
      output: '[main (root-commit) a1b2c3d] Initial commit',
      graphState: {
        title: 'First commit (made with git)',
        branches: [MAIN],
        commits: [{ id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] }],
      },
      highlightedCommitId: 'a1b2c3d',
      headLabel: 'HEAD',
    },
    {
      command: 'git checkout -b feature/search && git commit -m "Add search index"',
      vcs: 'git',
      description: 'Create a feature branch and commit with standard git tooling.',
      output: "Switched to a new branch 'feature/search'\n[feature/search b5c6d7e] Add search index\n 1 file changed, 30 insertions(+)",
      graphState: {
        title: 'git branch + commit',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'], bookmarks: ['feature/search'] },
        ],
      },
      highlightedCommitId: 'b5c6d7e',
      headLabel: 'HEAD',
    },
    {
      command: 'jj git init --colocate',
      vcs: 'jj',
      description: 'Wrap the existing git repo with jj. All git branches become jj bookmarks instantly — no migration needed.',
      output: 'Done. The colocated Git repo has been initialized.\nWorking copy now at: nxmkqvrs (empty)\nParent commit      : b5c6d7e Add search index',
      graphState: {
        title: 'jj wrapped around the git repo',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'], bookmarks: ['feature/search'] },
          { id: 'nxmkqvrs', label: '(empty working copy)', branch: 'feature/search', parents: ['b5c6d7e'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'nxmkqvrs',
      headLabel: '@',
    },
    {
      command: 'jj describe -m "Add search UI"',
      vcs: 'jj',
      description: 'Describe the working copy with jj. This is the equivalent of a git commit message — jj tracks it as part of the change.',
      output: 'Working copy now at: nxmkqvrs Add search UI\nParent commit      : b5c6d7e Add search index',
      graphState: {
        title: 'jj described the working copy',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'], bookmarks: ['feature/search'] },
          { id: 'nxmkqvrs', label: 'Add search UI', branch: 'feature/search', parents: ['b5c6d7e'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'nxmkqvrs',
      headLabel: '@',
    },
    {
      command: 'git log --oneline',
      vcs: 'git',
      description: 'Switch back to git and check the log. The jj working-copy change appears as a real git commit — both tools use the same object store.',
      output: 'nxmkqvrs (HEAD -> feature/search) Add search UI\nb5c6d7e Add search index\na1b2c3d (main) Initial commit',
      graphState: {
        title: 'git sees jj\'s commit',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'], bookmarks: ['feature/search'] },
          { id: 'nxmkqvrs', label: 'Add search UI', branch: 'feature/search', parents: ['b5c6d7e'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'nxmkqvrs',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit --allow-empty -m "Add search tests" --no-edit',
      vcs: 'git',
      description: 'Make another commit with plain git while jj is also set up. jj will pick this up automatically on the next jj command.',
      output: '[feature/search c7d8e9f] Add search tests\n 1 file changed, 45 insertions(+)',
      graphState: {
        title: 'git commit on top of jj\'s work',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'], bookmarks: ['feature/search'] },
          { id: 'nxmkqvrs', label: 'Add search UI', branch: 'feature/search', parents: ['b5c6d7e'] },
          { id: 'c7d8e9f', label: 'Add search tests', branch: 'feature/search', parents: ['nxmkqvrs'] },
        ],
      },
      highlightedCommitId: 'c7d8e9f',
      headLabel: 'HEAD',
    },
    {
      command: 'jj log',
      vcs: 'jj',
      description: 'jj sees the git commit immediately — no sync step needed. The log shows all commits from both tools together, with @ pointing to a new empty working copy on top.',
      output: '@  pqrstuvw (empty)\n│  (no description set)\n○  c7d8e9f Add search tests\n○  nxmkqvrs Add search UI\n○  b5c6d7e (feature/search) Add search index\n○  a1b2c3d (main) Initial commit',
      graphState: {
        title: 'jj sees git\'s commit too',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'], bookmarks: ['feature/search'] },
          { id: 'nxmkqvrs', label: 'Add search UI', branch: 'feature/search', parents: ['b5c6d7e'] },
          { id: 'c7d8e9f', label: 'Add search tests', branch: 'feature/search', parents: ['nxmkqvrs'] },
          { id: 'pqrstuvw', label: '(empty working copy)', branch: 'feature/search', parents: ['c7d8e9f'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'pqrstuvw',
      headLabel: '@',
    },
    {
      command: 'jj bookmark set feature/search -r @- && jj git push',
      vcs: 'jj',
      description: 'Move the feature/search bookmark to the last real commit (@- = parent of the empty WC), then push to the remote using jj. The remote sees standard git objects.',
      output: 'Updated bookmark feature/search to point to c7d8e9f\nBranch changes to push to origin:\n  Move bookmark feature/search from b5c6d7e to c7d8e9f\nPushed 1 bookmarks to origin',
      graphState: {
        title: 'Pushed to remote — full history from both tools',
        branches: [MAIN, FEAT_SEARCH],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] },
          { id: 'b5c6d7e', label: 'Add search index', branch: 'feature/search', parents: ['a1b2c3d'] },
          { id: 'nxmkqvrs', label: 'Add search UI', branch: 'feature/search', parents: ['b5c6d7e'] },
          { id: 'c7d8e9f', label: 'Add search tests', branch: 'feature/search', parents: ['nxmkqvrs'], bookmarks: ['feature/search', 'origin/feature/search'] },
          { id: 'pqrstuvw', label: '(empty working copy)', branch: 'feature/search', parents: ['c7d8e9f'], isWorkingCopy: true },
        ],
      },
      highlightedCommitId: 'pqrstuvw',
      headLabel: '@',
    },
  ],
};

// ── Scenario 5: Git Worktrees ─────────────────────────────────────────────────

const GIT_WORKTREES: Scenario = {
  id: 'git-worktrees',
  name: 'Git: Worktrees',
  description:
    'Git worktrees let you check out multiple branches simultaneously in separate directories, each with their own working copy — no stashing, no branch switching.',
  icon: '🗂️',
  tags: ['git', 'worktrees', 'parallel'],
  terminals: [
    { id: 'main', label: 'main worktree', cwd: '~/my-app' },
    { id: 'feature', label: 'feature worktree', cwd: '~/my-app-feat' },
  ],
  steps: [
    {
      command: 'git init my-app && cd my-app && git commit --allow-empty -m "Initial commit"',
      vcs: 'git',
      terminalId: 'main',
      description: 'Create the repository with an initial commit. There is only one worktree so far — the main one.',
      output: 'Initialized empty Git repository in /home/user/my-app/.git/\n[main (root-commit) a1b2c3d] Initial commit',
      graphState: {
        title: 'Single worktree: main',
        branches: [MAIN],
        commits: [{ id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main'] }],
      },
      highlightedCommitId: 'a1b2c3d',
      headLabel: 'HEAD',
    },
    {
      command: 'git worktree add -b feature ../my-app-feat',
      vcs: 'git',
      terminalId: 'main',
      description: 'Create a second worktree in a sibling directory, checked out to a new "feature" branch. Both worktrees share one .git directory.',
      output: "Preparing worktree (new branch 'feature')\nHEAD is now at a1b2c3d Initial commit",
      graphState: {
        title: 'Two worktrees, same starting commit',
        branches: [MAIN, FEAT],
        commits: [{ id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['main', 'feature'] }],
      },
      highlightedCommitId: 'a1b2c3d',
      headLabel: 'HEAD (both)',
    },
    {
      command: 'git commit --allow-empty -m "Update CI config"',
      vcs: 'git',
      terminalId: 'main',
      description: 'Commit in the main worktree. The feature worktree\'s branch pointer stays put — each worktree has its own HEAD.',
      output: '[main b3c4d5e] Update CI config',
      graphState: {
        title: 'Main worktree advanced; feature at a1b2c3d',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [], bookmarks: ['feature'] },
          { id: 'b3c4d5e', label: 'Update CI config', branch: 'main', parents: ['a1b2c3d'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'b3c4d5e',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit --allow-empty -m "Add search feature"',
      vcs: 'git',
      terminalId: 'feature',
      description: 'Meanwhile, in the feature worktree directory, commit independently. Both branches are advancing in parallel — no stashing needed.',
      output: '[feature f1c2d3e] Add search feature',
      graphState: {
        title: 'Both worktrees advancing in parallel',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'b3c4d5e', label: 'Update CI config', branch: 'main', parents: ['a1b2c3d'], bookmarks: ['main'] },
          { id: 'f1c2d3e', label: 'Add search feature', branch: 'feature', parents: ['a1b2c3d'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'f1c2d3e',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit --allow-empty -m "Fix security vulnerability"',
      vcs: 'git',
      terminalId: 'main',
      description: 'Back in the main worktree, address a security issue. Notice: no git stash, no branch switching — the feature worktree is completely undisturbed.',
      output: '[main c5d6e7f] Fix security vulnerability',
      graphState: {
        title: 'Parallel development continues',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'b3c4d5e', label: 'Update CI config', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'c5d6e7f', label: 'Fix security vulnerability', branch: 'main', parents: ['b3c4d5e'], bookmarks: ['main'] },
          { id: 'f1c2d3e', label: 'Add search feature', branch: 'feature', parents: ['a1b2c3d'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'c5d6e7f',
      headLabel: 'HEAD',
    },
    {
      command: 'git commit --allow-empty -m "Add search filters"',
      vcs: 'git',
      terminalId: 'feature',
      description: 'The feature worktree gets another commit. Both branches now have 2 commits past the common ancestor.',
      output: '[feature g4h5i6j] Add search filters',
      graphState: {
        title: 'Both branches 2 commits past fork',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'b3c4d5e', label: 'Update CI config', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'c5d6e7f', label: 'Fix security vulnerability', branch: 'main', parents: ['b3c4d5e'], bookmarks: ['main'] },
          { id: 'f1c2d3e', label: 'Add search feature', branch: 'feature', parents: ['a1b2c3d'] },
          { id: 'g4h5i6j', label: 'Add search filters', branch: 'feature', parents: ['f1c2d3e'], bookmarks: ['feature'] },
        ],
      },
      highlightedCommitId: 'g4h5i6j',
      headLabel: 'HEAD',
    },
    {
      command: 'git merge feature',
      vcs: 'git',
      terminalId: 'main',
      description: 'From the main worktree, merge the feature branch. This creates a merge commit with both branches\' work combined.',
      output: "Merge made by the 'ort' strategy.\n 2 files changed, 50 insertions(+)",
      graphState: {
        title: 'Merged from main worktree',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'b3c4d5e', label: 'Update CI config', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'c5d6e7f', label: 'Fix security vulnerability', branch: 'main', parents: ['b3c4d5e'] },
          { id: 'f1c2d3e', label: 'Add search feature', branch: 'feature', parents: ['a1b2c3d'] },
          { id: 'g4h5i6j', label: 'Add search filters', branch: 'feature', parents: ['f1c2d3e'], bookmarks: ['feature'] },
          { id: 'h7i8j9k', label: 'Merge branch feature', branch: 'main', parents: ['c5d6e7f', 'g4h5i6j'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'h7i8j9k',
      headLabel: 'HEAD',
    },
    {
      command: 'git worktree remove ../my-app-feat && git branch -d feature',
      vcs: 'git',
      terminalId: 'main',
      description: 'Remove the feature worktree directory and delete the branch. The merge commit keeps all history reachable — worktrees are truly just working copies.',
      output: 'Removing worktrees/my-app-feat\nDeleted branch feature (was g4h5i6j).',
      graphState: {
        title: 'Worktree removed; history intact',
        branches: [MAIN, FEAT],
        commits: [
          { id: 'a1b2c3d', label: 'Initial commit', branch: 'main', parents: [] },
          { id: 'b3c4d5e', label: 'Update CI config', branch: 'main', parents: ['a1b2c3d'] },
          { id: 'c5d6e7f', label: 'Fix security vulnerability', branch: 'main', parents: ['b3c4d5e'] },
          { id: 'f1c2d3e', label: 'Add search feature', branch: 'feature', parents: ['a1b2c3d'] },
          { id: 'g4h5i6j', label: 'Add search filters', branch: 'feature', parents: ['f1c2d3e'] },
          { id: 'h7i8j9k', label: 'Merge branch feature', branch: 'main', parents: ['c5d6e7f', 'g4h5i6j'], bookmarks: ['main'] },
        ],
      },
      highlightedCommitId: 'h7i8j9k',
      headLabel: 'HEAD',
    },
  ],
};

// ── Export ────────────────────────────────────────────────────────────────────

export const SCENARIOS: [Scenario, ...Scenario[]] = [
  GIT_BRANCH_MERGE,
  GIT_BRANCH_REBASE,
  JJ_STACKED,
  JJ_GIT_INTEROP,
  GIT_WORKTREES,
];
