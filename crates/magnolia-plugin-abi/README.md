# Magnolia Plugin ABI

A stable C ABI for Magnolia plugins, enabling cross-version compatibility and dynamic loading.

## ABI Version

**Current Version: 2**

Breaking changes require incrementing the version. Plugins built with an older ABI version may be rejected.

## Stability Guarantees

### Stable (Do Not Change)
- `PluginManifest` struct layout
- `ModuleRuntimeVTable` function signatures
- `SignalBuffer` struct layout
- `SignalType` enum values (0-7)
- Symbol names (`magnolia_plugin_*`)

### Extensible (Backwards Compatible)
- New optional symbol exports (e.g., `magnolia_plugin_get_schema`)
- New `SignalType` variants (numbering continues from 8+)
- New fields at END of `#[repr(C)]` structs

### Breaking Changes (Requires Version Bump)
- Changing existing function signatures
- Reordering struct fields
- Changing existing enum values
- Removing symbols

## Required Exports

Every plugin must export these symbols:

```c
// Plugin manifest
PluginManifest magnolia_plugin_manifest(void);

// Instance creation/destruction
void* magnolia_plugin_create(void);

// VTable for runtime callbacks  
const ModuleRuntimeVTable* magnolia_plugin_get_vtable(void);
```

## Optional Exports

```c
// Schema for port discovery (ABI v2+)
const ModuleSchemaAbi* magnolia_plugin_get_schema(void);
```

## Example Plugin

See `/examples/hello_plugin` for a complete Rust implementation.

## Hot-Reload Support

The host application supports hot-reloading plugins during development:

1. `pre_unload()` is called on the old instance
2. New shared library is loaded
3. `post_reload()` is called on the new instance

Plugins should be stateless or serialize state in `get_state()` for restoration.
