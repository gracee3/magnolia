# Project Organization and Legacy Source Plan

Status: proposed cross-project boundary  
Last reviewed: 2026-07-21

## Recommendation

Run four independent tracks:

| Repository | Responsibility | Relationship to Magnolia |
|---|---|---|
| `magnolia` | Headless runtime, audio/DSP, recording, STT, visual modules, process supervision, semantic commands, agent adapter | Foundational tool; no astro/tarot dependency |
| `astraeus` | Astrology domain, ephemeris adapters, aspects, chart specifications, validation fixtures | Optional external consumer/integration |
| `tarot-rs` | Deck-independent tarot domain, spreads, secure shuffle, readings, provenance | Independent; no Magnolia dependency initially |
| `oracle-studio` | People, sessions, journal, outcomes, storage/privacy, combined astro/tarot workflows and UI | Depends on Astraeus and tarot; optionally calls Magnolia |

Do not create a combined Oracle core containing astrology and tarot. Their correctness rules, licensing, fixtures, and release cadence are different. `oracle-studio` is the selected application name and does not own either calculation core.

The dependency direction is:

```text
astraeus --------+
                 +--> oracle-studio --optional--> magnolia client/API
tarot-rs --------+

magnolia ------------------------------------------------ independent
```

Magnolia may later provide generic artifacts such as recordings and transcripts. Oracle Studio stores references or imports artifacts through a public API. Magnolia never imports Oracle person, astrology, or tarot types into its kernel.

## GitHub inventory relevant to astrology and tarot

The GitHub account is `gracee3`. No repository named for tarot or Oracle currently exists.

### Recommended active source

- `gracee3/aphrodite-rust`: public AGPL Rust workspace, approximately 82 tracked files. This is the immutable legacy source for Astraeus, pinned initially at commit `e8d9580f119fcf39ccc39d7ac9265d9689f1c274`. It already separates core, API, Slint, and WASM concerns, although its implementation status and calculation validation need correction.

### Useful reference repositories

- `gracee3/aphrodite-d3`: public MIT TypeScript/D3 renderer. Use as a visual behavior and chart-interaction reference; do not make the Rust core depend on it.
- `gracee3/crius-ephemeris-core`: public MIT Python interface/types package. Its adapter boundary and validation concepts are useful requirements for Astraeus.
- `gracee3/crius-swiss`: public Python Swiss adapter. Use as comparison material and possible fixture source. Reconcile its actual license file/metadata before copying code.
- `gracee3/crius-jpl`: public Python JPL adapter. Use its provider-separation and test ideas as reference. Its README's mixed Swiss/JPL licensing claims require review before copying code.
- `gracee3/gaia`: private full-stack astrology workspace with a structured backend, API types, wheel definitions, and renderer packages. This is the strongest source for product requirements and prior API/UI behavior.
- `gracee3/ouranos`: private earlier FastAPI/React implementation with extensive chart controls and wheel rendering. Use as a feature and interaction inventory.
- `gracee3/astrolog-ui`: private Astrolog wrapper/UI experiment. Use for command-builder requirements and workflow fixtures, not as an application base.
- `gracee3/swetest-api`: private small Python wrapper. Use only as a validation/reference harness if its expected outputs are trustworthy.

### Archive-only material

- `gracee3/astro`: private, approximately 38 MB, mixing scripts, ephemeris files, generated CSV/PNG output, Python caches, Astrolog data, and personal chart material. Never use this repository wholesale as a clean seed.
- `gracee3/talisman`: private earlier Magnolia-shaped snapshot, not a tarot implementation despite its name.
- `gracee3/Astrolog`: public fork of Astrolog. Keep as an independent external reference; do not vendor the full program into Astraeus.

## Clean-start policy

GitHub source archives are appropriate as history-free reference snapshots, but they should not be copied wholesale into a new repository.

For each legacy repository selected as a source:

1. Record the repository URL, visibility, source commit SHA, license, and archive checksum in a local import manifest.
2. Download the GitHub-generated source archive for that exact commit outside the new repository.
3. Scan it for credentials, personal data, generated output, binary assets, caches, and incompatible licenses.
4. Write requirements and golden fixtures from the observed behavior.
5. Copy only deliberately selected source or data files whose provenance and license are clear.
6. Commit the selected material as a new clean commit with an attribution note; do not preserve old Git metadata in the new repository.
7. Keep the original GitHub repository unchanged or mark it archived after the new implementation is demonstrably complete.

Do not delete or rewrite the old repositories merely to obtain a clean history. A new repository or an intentionally clean new worktree is safer, and the old URL remains useful evidence.

Do not commit downloaded ZIP/tar archives into the new source repositories. Store them in a private backup location with checksums, or rely on the immutable Git commit while keeping a separate offline backup.

## Astraeus recommendation

Use the new, empty `gracee3/astraeus` repository as the clean active astrology project. Keep `gracee3/aphrodite-rust` unchanged as the legacy source and provenance record. Do not bulk-copy the old workspace into Astraeus; import reviewed components and fixtures deliberately from the pinned source commit.

Build Astraeus in staged checkpoints:

1. Start with documentation, licensing, a pinned Rust toolchain, and an empty `astraeus-core` crate.
2. Define calculation inputs, outputs, errors, and an ephemeris adapter trait before importing an adapter implementation.
3. Import requirements and verified fixtures from Aphrodite, Crius, Gaia, Ouranos, `swetest`, and Astrolog rather than copying entire implementations.
4. Implement or selectively import the Swiss adapter with correct path setup, ayanamsas, speed flags, errors, and global-state handling.
5. Establish deterministic golden tests before importing aspects, chart specifications, Jyotish, UI, or API code.
6. Treat all legacy README completion claims as historical until Astraeus tests substantiate them.

The first Astraeus commit should contain only the project handoff, README, and AGPL license. This gives the Track B agent a clean boundary and prevents unvalidated code from becoming the new baseline by accident.

## Track B handoff: Astraeus

### Mission

Build a small, validated Rust astrology engine whose calculation results are deterministic, complete, and independently reproducible. Do not build Oracle Studio, tarot, Magnolia integration, or a GUI during the first validation checkpoint.

### Repositories and evidence

- Active clean repository: `git@github.com:gracee3/astraeus.git`.
- Legacy Rust source: `https://github.com/gracee3/aphrodite-rust` at `e8d9580f119fcf39ccc39d7ac9265d9689f1c274`.
- Local legacy checkout, when available: `/home/emmy/git/aphrodite-rust`.
- Additional requirements/fixture sources: `aphrodite-d3`, `crius-ephemeris-core`, `crius-swiss`, `crius-jpl`, private `gaia`, private `ouranos`, private `astrolog-ui`, private `swetest-api`, and the Astrolog fork.

### Known legacy defects

The legacy workspace is evidence, not a validated engine:

- The headless library checks, but its test target does not compile because Tokio test macro/runtime features are incomplete.
- API integration tests hard-code `/usr/local/share/swisseph`; the prior audit produced 45 failures and 15 ignored tests.
- The configured ephemeris path is checked but not passed to Swiss Ephemeris.
- Sidereal mode is cached locally but never applied to the library.
- Several ayanamsa numeric mappings are incorrect.
- Planet speed/retrograde is read without requesting the Swiss speed flag.
- Requested planets that fail calculation are silently omitted, permitting partial success.
- Precision tests are ignored `assert!(true)` placeholders, and another ephemeris test uses the current time instead of a fixed fixture.
- Swiss Ephemeris configuration is global and needs serialization or process isolation.
- Slint/WASM/API scaffolds and README completion claims are ahead of demonstrated behavior.

### First checkpoint

1. Create the minimal `astraeus-core` crate with no UI, HTTP, database, Magnolia, or Oracle dependencies.
2. Define validated UTC time, geographic location, object, zodiac, ayanamsa, house-system, position, house, and calculation-error types.
3. Define an ephemeris adapter trait and a deterministic mock adapter.
4. Add fixed golden-fixture file formats and a harness capable of comparing Astraeus output to `swetest`/Astrolog reference output.
5. Document Swiss Ephemeris licensing and global-state constraints before adding the native dependency.
6. Add the Swiss adapter only after the contract and failure semantics have tests.

Checkpoint exit condition: the clean workspace builds and tests without Swiss data files, while an explicitly selected integration suite validates at least one tropical and one sidereal chart against pinned external reference output.

### Non-goals for the first checkpoint

- No Axum API, Slint, WASM, D3, or native wheel renderer.
- No database, people/client records, journal, or encryption.
- No tarot or Oracle Studio types.
- No Magnolia module or MCP adapter.
- No card, chart, transit, progression, or Jyotish feature expansion before the ephemeris foundation passes golden tests.

## Tarot recommendation

Create a new `tarot-rs` repository with no dependency on Magnolia, Astraeus, a UI toolkit, a database, or an LLM provider.

The first crate owns only:

- Canonical card identities and suits/ranks/archetypes.
- Deck definitions separate from copyrighted artwork and guidebook text.
- Spread definitions and position semantics.
- Draws, orientation/reversal, and reading records.
- Cryptographically secure unbiased shuffle using OS randomness and Fisher-Yates.
- Algorithm version and provenance sufficient to audit a generated reading.
- Serialization, validation, and deterministic tests.

Defer people/client records, encrypted storage, AI interpretation, card recognition, licensed packs, synchronization, and GUI work to Oracle Studio or later dedicated crates.

Before scaffolding, extract the user's tarot requirements from existing notes into a short domain specification. No tarot source repository was found on GitHub, so there is no implementation to preserve.

## Oracle Studio recommendation

Create `oracle-studio` only after Astraeus has validated calculations and `tarot-rs` has a stable domain model. Oracle Studio owns:

- People and client profiles.
- Sessions and questions.
- Notes, observations, hypotheses, predictions, corrections, and outcomes.
- References to immutable astrology calculations and tarot readings.
- Recording/transcript artifact references.
- Local persistence, migrations, privacy, export, and eventual encrypted synchronization.
- Agent-assisted correlation and editable memory with source provenance.

Oracle Studio can begin as a CLI or simple native application. It does not need Magnolia to prove its domain model. A Magnolia adapter is added only when recording, transcription, shared visualization, or agent operation provides concrete value.

## Immediate sequence

1. Finish Magnolia Phase 0 and the Sherpa/OpenAI recording slice without astrology or tarot work.
2. In parallel as a separate track, bootstrap clean `astraeus`, implement its validation-first core, and collect selected legacy fixtures from the pinned Aphrodite source.
3. Write the tarot domain requirements, then scaffold a new `tarot-rs` repository.
4. Delay creation of Oracle Studio until the two domain APIs are small and demonstrably stable.
5. Define cross-project integration using ordinary Rust library APIs and versioned serialized artifacts before considering Magnolia modules or MCP tools.

## Decisions still needed

1. Whether `tarot-rs` should initially be public AGPL, public permissive, or private. This affects later Oracle Studio licensing and deck-pack contributions.
2. Where private source archives and personal astrology fixtures should be stored. They should not be placed in public test data.
