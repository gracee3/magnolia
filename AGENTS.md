# Talisman Architecture for Agents

This workspace is structured to separate concerns into distinct "Spirits".

## The Crates (The Libraries)

- **Aphrodite (`crates/aphrodite`)**: **Time**. Handles Astrology, Ephemerides, and temporal calculations.
- **Logos (`crates/logos`)**: **Intent**. Handles Input processing, Hardware control (Caps Lock), and Semantic Hashing.
- **Kamea (`crates/kamea`)**: **Geometry**. Handles Grid mathematics, Sigil path generation, and visual shapes.

## The App (The Body)

- **Daemon (`apps/daemon`)**: **The Vessel**. A Nannou-based visualizer that orchestrates the crates.

## Metaphor
We are forging a digital talisman.
1. We capture **Intent** (Logos).
2. We Salt it with **Time** (Aphrodite).
3. We Seal it in **Geometry** (Kamea).
