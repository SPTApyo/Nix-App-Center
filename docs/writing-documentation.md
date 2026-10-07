# Writing documentation

Conventions for adding to docs/, whether you write the doc yourself or ask a
model to write it.

## Where docs go

Project and coding documentation goes in docs/, whoever the reader is. Humans
and agents want the same thing: what this is, how it works, where the code is.

AGENTS.md only says how to operate as an agent here (verification, safety,
which doc to load). If you are about to explain how a subsystem works in
AGENTS.md, it belongs in docs/.

These files stay at the repository root: README.md, CONTRIBUTING.md,
SECURITY.md, AGENTS.md.

## Rules

- Add every new doc to [README.md](README.md) with one line saying which
  question it answers.
- Kebab-case file names named after the topic: storage.md, not
  storage-guide.md.
- First line is the title, then one or two sentences on what the doc answers.
- Cite source paths from the repository root: crates/core/src/config_attach.rs.
- Write in English, short sentences, plain words. No em dashes.
- Explain why. The code already says what.
- Update the doc in the same pull request as the behavior it describes.
