export interface SkillFile {
  path: string;
  size: number;
  binary: boolean;
  githubUrl: string;
  rawUrl: string;
}

export interface Skill {
  id: string;
  name: string;
  description: string | null;
  source: string;
  category: string | null;
  license: string | null;
  proprietary?: boolean;
  path: string;
  url: string;
  route: string;
  files: SkillFile[];
}

export interface RelatedSkill {
  name: string;
  route: string;
  author: string;
  description: string;
}

// Built at build time by skillPageProps() in plugins/skillsPlugin.js.
export interface SkillPageProps {
  skill: Skill;
  title: string;
  metaDescription: string;
  author: string;
  authorRoute: string | null;
  body: string | null;
  licenseUrl: string | null;
  manualOnly: boolean;
  trigger: string | null;
  install: { folder: string; claudeCode: string; codex: string } | null;
  related: RelatedSkill[];
}
