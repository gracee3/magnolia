# ADR 0004: Leptos CSR over loopback with Chromium

Status: Accepted, 2026-08-27

## Decision

Use Leptos 0.8 CSR/Trunk, following the official [Leptos CSR/Trunk guide](https://book.leptos.dev/getting_started/index.html), served over authenticated loopback, and launch a dedicated Chromium app window/profile. Tauri is not the first shell.

## Consequences

Linux Tauri uses WebKitGTK; see [Tauri webview versions](https://v2.tauri.app/reference/webview-versions/). A future Tauri adapter uses the same protocol and commands/channels instead of generic events for throughput, following [Tauri frontend communication guidance](https://v2.tauri.app/develop/calling-frontend/). There is no Nannou/Leptos bridge.
