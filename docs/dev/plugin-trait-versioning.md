---
project: openpulsehf
doc: docs/plugin-trait-versioning.md
status: living
last_updated: 2026-04-24
---

# Plugin Trait Versioning and Compatibility

## Purpose

This document defines formal compatibility guarantees for the `ModulationPlugin` trait and related plugin interfaces. It ensures that:

- Existing plugins continue to work across framework versions when possible.
- Breaking changes are explicit, predictable, and documented.
- Plugin authors understand their upgrade path.
- The framework can evolve without abandoning deployed plugins.

## Scope

This policy applies to:
- `openpulse_core::plugin::ModulationPlugin` trait (the primary plugin contract).
- `openpulse_core::plugin::PluginInfo` struct (plugin metadata).
- `openpulse_core::plugin::ModulationConfig` struct (configuration passed to plugins).
- Related types exported by `openpulse_core::plugin` module.

This policy does **not** cover:
- Internal implementation details (private modules, non-public types).
- Optional ecosystem extensions (experimental traits, feature-gated APIs).
- Application-level APIs outside the plugin crate.

## Trait Versioning Scheme

### Version Format

Plugin trait compatibility is tracked via **semantic versioning** applied to the plugin interface as a whole:

```
<major>.<minor>.<patch>
```

The current trait version is **`1.0.0`**.

### Trait Version Identification

The trait version is:
- **Canonical source**: hardcoded in `crates/openpulse-core/src/lib.rs` as the constant `PLUGIN_TRAIT_VERSION: &str = "1.0.0"`.
- **Published in**: each framework release's `docs/releasenotes.md`.
- **Declared by plugins**: in `PluginInfo::trait_version_compatibility` (see § Plugin Declaration below).

## Compatibility Rules

### Major Version Increments (Breaking)

A **major version bump** indicates one or more breaking changes. Plugins built against prior major versions require updates.

Breaking changes include:

- **Removal** of a required trait method or public function.
- **Signature change** of a required method (parameter types, return type).
- **Semantics change**: modified behavior of existing methods (e.g., `modulate` now requires the input to be a specific encoding; previously it didn't).
- **New required methods**: trait adds a new method that existing implementers must provide.
- **Type changes** in `PluginInfo` or `ModulationConfig` that affect plugin compilation.
- **Error contract change**: new error cases from existing methods that plugins must now handle.

**Example**:

```rust
// Version 1.0.0
pub trait ModulationPlugin: Send + Sync {
    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<Vec<f32>, ModemError>;
}

// Version 2.0.0 — breaking change: return type changed
pub trait ModulationPlugin: Send + Sync {
    fn modulate(&self, data: &[u8], config: &ModulationConfig) -> Result<ModulatedSignal, ModemError>;
    // ^^ return type is now ModulatedSignal, not Vec<f32>
}
```

### Minor Version Increments (Backward-compatible)

A **minor version bump** adds new capabilities without breaking existing implementations. Plugins built against prior versions continue to work.

Backward-compatible changes include:

- **New optional methods** with a provided default implementation (via trait extension or blanket impl).
- **New fields** added to `PluginInfo` or `ModulationConfig` with sensible defaults and no impact on existing plugins.
- **New error variants** added to `ModemError` that existing plugins need not emit (i.e., errors that the framework can emit but plugins are not required to understand).
- **New helper functions** added to the plugin module (non-breaking).
- **Documentation improvements** without semantic change.

**Example**:

```rust
// Version 1.0.0
pub struct PluginInfo {
    pub name: String,
    pub version: String,
}

// Version 1.1.0 — backward-compatible: new optional field
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub license: Option<String>,  // New, defaultable via builder or serde default
}
```

### Patch Version Increments (Bug fixes)

A **patch version bump** includes bug fixes, performance improvements, or clarifications with no interface change.

- No changes to trait signatures, method lists, or type definitions.
- Clarifications to method documentation that do not change semantics.
- Internal refactors in the framework that do not affect plugin behavior.

---

## Plugin Declaration

Plugins must declare trait version compatibility in their `PluginInfo`. This is enforced at **registration time** by the framework.

### PluginInfo Extension

`PluginInfo` is extended with:

```rust
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub supported_modes: Vec<String>,
    
    /// Plugin trait version requirement.
    /// Specifies the trait version(s) this plugin is compatible with.
    /// Format: "<major>.<minor>", e.g. "1.0", "2.1"
    /// Multiple versions may be supported in future; for now, exactly one.
    pub trait_version_required: String,
}
```

### Valid Trait Version Declarations

A plugin author specifies the **minimum major.minor version** of the trait their plugin was built against.

**Rules**:

- The declared version must be in the form `"<major>.<minor>"` (no patch version).
- The declared version must be **less than or equal** to the current framework trait version.
- The plugin can work with any **patch version** of the declared major.minor (e.g., `"1.0"` works with framework `1.0.0`, `1.0.1`, `1.0.2`, …).
- If the framework's **major version** is newer than the plugin's declared version, the plugin is **incompatible** (registration fails).
- If the framework's **minor version** is older than the plugin's declared version, the plugin is **incompatible** (registration fails).

**Example**:

```rust
// Plugin built for trait version 1.0
let plugin = BpskPlugin::new();
let info = plugin.info();
assert_eq!(info.trait_version_required, "1.0");

// Framework is at trait version 1.0.0 ✓ compatible
// Framework is at trait version 1.1.0 ✓ compatible (minor bump is backward-compatible)
// Framework is at trait version 2.0.0 ✗ incompatible (major bump, breaking changes)
```

---

## Registration Validation

When a plugin is registered with `PluginRegistry::register()`, the framework validates trait version compatibility:

```rust
impl PluginRegistry {
    /// Register a plugin, validating trait version compatibility.
    pub fn register(&mut self, plugin: Box<dyn ModulationPlugin>) -> Result<(), PluginError> {
        let info = plugin.info();
        
        // Parse plugin's declared trait version
        let declared = parse_trait_version(&info.trait_version_required)?;
        let current = PLUGIN_TRAIT_VERSION;
        
        // Check compatibility
        if !is_compatible(declared, current) {
            return Err(PluginError::IncompatibleTraitVersion {
                plugin: info.name.clone(),
                required: info.trait_version_required.clone(),
                current: current.to_string(),
            });
        }
        
        self.plugins.push(plugin);
        Ok(())
    }
}

fn is_compatible(plugin_required: (u32, u32), framework_current: (u32, u32, u32)) -> bool {
    let (p_major, p_minor) = plugin_required;
    let (f_major, f_minor, _f_patch) = framework_current;
    
    // Compatible if framework major matches AND framework minor >= plugin minor
    p_major == f_major && f_minor >= p_minor
}
```

---

## Migration Path for Breaking Changes

When a breaking change is necessary, the following process is followed:

### 1. Planning phase

- Breaking change is proposed in a GitHub issue with label `breaking-change-proposal`.
- Discussion includes: justification, impact on known plugins, transition timeline.
- Decision is approved by at least one maintainer before work begins.

### 2. Development phase

- Breaking change is implemented on a feature branch (e.g., `feat/plugin-trait-v2`).
- **Dual compatibility layer** (if practical): framework may accept both old and new implementations during a grace period (e.g., one minor release).
- **Examples and migration guide** are drafted concurrently.

### 3. Release phase

- **New major version** is released (e.g., `2.0.0`).
- `PLUGIN_TRAIT_VERSION` is bumped to `"2.0"`.
- `docs/releasenotes.md` includes a dedicated section:
  - **Breaking changes**: detailed list of what changed.
  - **Migration guide**: step-by-step update instructions for plugin authors.
  - **Example code**: before/after for common patterns.
  - **Deprecated patterns**: if a compatibility layer is in place, document its lifespan.

### 4. Deprecation phase (if applicable)

- If a compatibility layer exists, a second minor release may retain the old interface as deprecated.
- Deprecation warnings are emitted at registration time for old-style plugins.
- Guidance directs plugin authors to upgrade within a specific timeframe (e.g., 2–3 minor releases).
- The old interface is removed in the next major version.

---

## Release Notes Template

Breaking changes and trait updates are documented following this template in `docs/releasenotes.md`:

```markdown
### Plugin trait version: 2.0

#### Breaking changes

- Removed `ModulationPlugin::supports_mode()` helper method (plugins now compute this inline).
- Added required method: `ModulationPlugin::describe_modes() -> Vec<ModeDescriptor>` (replaces `supported_modes` field in PluginInfo).

#### Migration guide

1. Remove the `supports_mode()` call from your plugin's use of the trait.
2. Implement `describe_modes()` to return a `Vec<ModeDescriptor>` with extended metadata:
   ```rust
   fn describe_modes(&self) -> Vec<ModeDescriptor> {
       vec![
           ModeDescriptor {
               mode: "BPSK31".to_string(),
               baud_rate: 31.25,
               bandwidth_hz: 31,
               // ... additional fields
           },
       ]
   }
   ```
3. Remove the `supported_modes` field from your `PluginInfo`.
4. Update `trait_version_required` in your `PluginInfo` to `"2.0"`.
5. Test registration against the new framework.

#### Example

**Before** (trait version 1.0):
```rust
impl ModulationPlugin for BpskPlugin {
    fn info(&self) -> &PluginInfo {
        &PluginInfo {
            supported_modes: vec!["BPSK31".into()],
            trait_version_required: "1.0".into(),
            …
        }
    }
    fn supports_mode(&self, mode: &str) -> bool { … }
}
```

**After** (trait version 2.0):
```rust
impl ModulationPlugin for BpskPlugin {
    fn info(&self) -> &PluginInfo {
        &PluginInfo {
            trait_version_required: "2.0".into(),
            …
        }
    }
    fn describe_modes(&self) -> Vec<ModeDescriptor> { … }
}
```
```

---

## Compatibility Matrix

| Framework version | Plugin `trait_version_required` | Outcome |
|---|---|---|
| 1.0.0 | `"1.0"` | ✓ Compatible |
| 1.1.0 | `"1.0"` | ✓ Compatible (minor is additive) |
| 1.2.3 | `"1.0"` | ✓ Compatible |
| 1.0.0 | `"1.1"` | ✗ Incompatible (framework minor too old) |
| 2.0.0 | `"1.0"` | ✗ Incompatible (major mismatch) |
| 2.0.0 | `"2.0"` | ✓ Compatible |
| 2.1.0 | `"2.0"` | ✓ Compatible |

---

## Stability Guarantees

The OpenPulse project commits to the following:

- **Within a major version**: trait is stable. No breaking changes during 1.x, 2.x, etc.
- **Before 1.0.0**: trait versioning may be less strict as the project is still evolving.
- **Deprecation notices**: when a feature is slated for removal, it is deprecated for **at least one minor version** before removal.
- **Notice period**: breaking changes are announced **at least one release in advance** in release notes.

---

## Audit and Observability

Plugin version compatibility should be observable:

- Registry startup logs include: `Loaded plugin BPSK (version X.Y, trait version required 1.0, framework trait version 1.1) ✓`.
- On incompatibility, error message includes: plugin name, required trait version, current framework trait version, and pointer to migration guide.
- A command (e.g., `openpulse-cli plugins list --compatibility`) lists all registered plugins with their trait versions.

---

## Future Extensions

The following areas may be refined in future trait versions:

- **Plugin capabilities bitmap**: plugins declare optional features (e.g., `supports_adaptive_coding`) to avoid adding new required methods.
- **ABI stability**: if plugins may be distributed as pre-compiled binaries (.so/.dll), ABI versioning becomes a concern (beyond this document).
- **Trait dependencies**: if plugins depend on other plugins, a dependency resolution mechanism may be needed.

