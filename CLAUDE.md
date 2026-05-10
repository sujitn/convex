# Convex — agent context

The agent scaffolding (Claude memory, prompts, plans, internal reconciliation
notes) for this repo lives in a separate private repo and is mounted at `.ai/`
as a git submodule.

If `.ai/` is empty, you do not have access — that is expected for public clones.
If you do have access, run:

    git submodule update --init --recursive

The active agent context file is `.ai/CLAUDE.md`.
