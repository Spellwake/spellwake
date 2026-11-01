# Repository installation

- Upstream: https://github.com/leonardomso/rust-skills
- Revision: `fd2a861ab0406a4ac536a55274d14ea6fd1ca9c9`
- Skill version: 1.5.1
- Installed: November 1, 2026, using the Codex skill-installer helper at an exact revision.
- License: MIT; upstream LICENSE retained.
- Canonical copy: `.agents/skills/rust-skills/`.
- Claude discovery: `.claude/skills/rust-skills` is a relative symlink to this copy.
- Upstream AGENTS.md and CLAUDE.md symlinks restored after archive extraction.
- Project policy and check commands remain in the root AGENTS.md.

Reviewed alongside The Prompt Shelf's Rust workflow guide:
https://thepromptshelf.dev/blog/claude-code-rust-projects-guide-2026/
Its template informed the root workflow; it was not copied wholesale. Spellwake
retains its existing error types, Rust edition, lockfiles, safe-code policy and
test assertions. No runtime dependency, global tool, or compiler was installed.

For updates, review an explicit upstream revision and its rules before replacing
this vendored copy. Keep the license, this provenance record and discovery link.
