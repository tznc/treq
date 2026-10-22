---
sidebar_position: 9
---

# Installing Skills from the Library

_Browse the Treq skill registry, verify the download, and choose where each skill lives._

Open **Settings** and select the **Skills** tab. That tab is present when **Skills installation** is on under Settings → Feature Preview.

Treq loads the public catalog from [treq.dev/skills](https://treq.dev/skills). Search by name or description. **Install level** next to search limits the list to skills already installed for the application, the current repository, or both.

## Install a skill

Each catalog row has **Install…**. The dialog explains the two store locations:

- **Install for repository** (primary) writes the skill under `.treq/skills/` in the current repository. Only that repository uses it.
- **Install for application** writes the skill into your Treq application data directory. Every repository on this machine can use it.

Treq downloads every file in the skill folder and computes a SHA-256 checksum of the file paths and contents. When the catalog includes a checksum, Treq refuses the install if the hashes differ.

Proprietary catalog entries stay listed. You cannot install them from Treq. Open the source link instead.

## Change install level

After a skill is installed, **Install level** on the right of the card switches it between application and repository. Treq moves the stored files and keeps the recorded checksum.

The trash control in the upper right of the card uninstalls the stored pack. Hover it for the Uninstall tooltip.

## New workspaces

Creating a workspace copies every installed library skill into:

- `.agents/skills/<skill-name>/` (Codex and Cursor Agent)
- `.claude/skills/<skill-name>/` (Claude Code)

Treq marks those copies with `.treq-generated`. It does not overwrite a folder that already exists without that marker. Each generated copy also gets its own `.gitignore`, so Git ignores it without changes to your repository's `.gitignore`; user-owned skills remain available to commit.

Application-level skills copy into workspaces for every repository. Repository-level skills copy only for that repository.
