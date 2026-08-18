# Contributor and agent guidance

Magnolia is experimental signal-processing research. The tree contains a Rust
workspace with modular runtime, plugin, audio, text, caption, and visual-host
components, but README and planning feature lists include intended behavior that
is not uniformly hardened or independently validated. Treat implementation and
tests—not aspirational prose—as the authority for current capability.

Before changing implementation, read `README.md`,
`docs/MAGNOLIA_REFOUNDATION_PLAN.md`, `docs/PHASE_0_INVENTORY.md`, and the
relevant crate manifests and tests. Read `docs/STT_BACKEND_PLAN.md` before
caption or speech-backend work.

## Validation boundary

No sufficiently narrow ordinary check has yet been reviewed for this workspace.
For instruction-only changes, run:

```bash
git diff --check
```

Before changing code, identify a bounded crate or package check and record what
it excludes. Do not download models or corpora, access audio devices, run live
capture, execute benchmarks, build plugins, start the visual host, or use a GPU
without separate explicit authorization.

## Scope, provenance, and delivery

- Magnolia remains domain-neutral and independent of astrology, tarot, people,
  client, journal, and encrypted-vault types.
- Do not claim plugin ABI stability, sandboxing, signing, hot reload, real-time
  performance, recognition accuracy, or GPU acceleration without tests and
  evidence for the exact path.
- Never commit recordings, transcripts, model/corpus payloads, `.env` files,
  secrets, local paths, raw host captures, or unjustified generated binaries.
  Record copied/adapted code, assets, model/corpus terms, and AI assistance.
- Use a focused feature branch. Commit and push the validated change and open a
  pull request; incomplete or higher-risk work stays draft.
- After publication, send the exact commit, PR, validation, outcome, risks, and
  next action to the repository's external coordination record. Do not claim
  completion until that remote handoff is verified.
