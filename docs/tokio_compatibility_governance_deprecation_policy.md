# Compatibility Governance and Deprecation Policy

**Bead**: `asupersync-2oh2u.11.6` ([T9.6])
**Program**: `asupersync-2oh2u` ([TOKIO-REPLACE])
**Date**: 2026-03-04
**Purpose**: Define compatibility governance, API stability commitments,
deprecation policy, and breaking-change management for Tokio-replacement
surfaces across all release channels.

---

## 1. Scope

This policy governs how API compatibility is maintained, how deprecations are
communicated, and how breaking changes are managed across the replacement
surface lifecycle. It is grounded in:

- Release channels from `asupersync-2oh2u.11.5` (T9.5)
- Migration lab outcomes from `asupersync-2oh2u.11.10` (T9.10)
- Compatibility matrix from `asupersync-2oh2u.11.3` (T9.3)

Prerequisites:
- `asupersync-2oh2u.11.10` (T9.10: migration lab KPIs)
- `asupersync-2oh2u.11.5` (T9.5: release channels)

Downstream:
- `asupersync-2oh2u.11.8` (T9.8: replacement claim RFC)

---

## 2. Compatibility Tiers

### 2.1 API Stability Levels

| Tier | Guarantee | Deprecation Notice | Removal Horizon | Applies To |
|------|-----------|-------------------|----------------|------------|
| Stable | Full semver | >= 2 minor releases | Next major only | GA surfaces |
| Provisional | Semver-soft | >= 1 minor release | Next minor allowed | Beta surfaces |
| Experimental | None | Best-effort | Any release | Alpha surfaces |
| Internal | None | None | Any commit | Private APIs |

### 2.2 Compatibility Dimensions

| Dimension ID | Name | Description | Enforcement |
|-------------|------|-------------|-------------|
| CD-01 | Source compatibility | Code compiles without changes | CI gate |
| CD-02 | Binary compatibility | ABI preserved across patch releases | Symbol checks |
| CD-03 | Behavioral compatibility | Observable behavior unchanged | E2E tests |
| CD-04 | Performance compatibility | Latency/throughput within budget | Benchmark gate |
| CD-05 | Wire compatibility | Protocol/serialization format preserved | Conformance tests |
| CD-06 | Configuration compatibility | Config keys and defaults preserved | Schema validation |

---

## 3. Deprecation Process

### 3.1 Deprecation Lifecycle

```text
PROPOSAL ──→ REVIEW ──→ APPROVED ──→ DEPRECATED ──→ REMOVED
   │           │           │             │              │
   │           │           │             │              └─ Next major release
   │           │           │             └─ #[deprecated] + migration guide
   │           │           └─ Governance board approval
   │           └─ Impact assessment complete
   └─ RFC filed with rationale
```

### 3.2 Deprecation Notice Requirements

| Requirement ID | Description |
|---------------|-------------|
| DN-01 | `#[deprecated(since, note)]` attribute on all deprecated items |
| DN-02 | Migration guide with before/after code examples |
| DN-03 | Changelog entry describing deprecation and rationale |
| DN-04 | Compiler warning with actionable replacement suggestion |
| DN-05 | Deprecation notice minimum duration per stability tier |

### 3.3 Deprecation Impact Assessment

Before deprecating any Stable or Provisional API:

| Step | Action | Owner |
|------|--------|-------|
| DIA-01 | Usage analysis across known consumers | Track lead |
| DIA-02 | Migration complexity estimate (FK-01 KPI) | QA lead |
| DIA-03 | Performance impact of replacement path | Performance engineer |
| DIA-04 | Compatibility matrix update | Governance board |
| DIA-05 | Migration cookbook entry | Documentation lead |

---

## 4. Breaking Change Management

### 4.1 Breaking Change Classification

| Class | Description | Allowed In | Approval Required |
|-------|-------------|-----------|-------------------|
| BC-01 | Type signature change | Major only | Governance board |
| BC-02 | Behavioral change (observable) | Major only | Governance board + RFC |
| BC-03 | Default value change | Minor (with deprecation) | Track lead |
| BC-04 | Feature removal | Major only | Governance board |
| BC-05 | Wire format change | Major only | Governance board + RFC |
| BC-06 | Performance regression > 10% | Blocked until fixed | Track lead |

### 4.2 Breaking Change RFC Process

1. **File RFC**: Author submits breaking change proposal with:
   - Technical rationale and alternatives considered
   - Impact assessment on known consumers
   - Migration path with estimated effort
   - Timeline and deprecation schedule

2. **Review period**: Minimum 14 days for Stable APIs, 7 days for Provisional

3. **Governance vote**: Quorum of 3+ governance board members

4. **Implementation**: Breaking change lands with:
   - Migration guide
   - Deprecation warnings in preceding release
   - Automated migration tool where feasible

---

## 5. Governance Board

### 5.1 Composition

| Role | Responsibility | Vote Weight |
|------|---------------|-------------|
| Program Lead | Final authority on cross-track decisions | 2 |
| Track Leads (T2-T7) | Domain expertise for affected tracks | 1 each |
| QA Lead | Quality and test coverage impact | 1 |
| Community Representative | User impact assessment | 1 |

### 5.2 Decision Thresholds

| Decision Type | Threshold | Quorum |
|--------------|-----------|--------|
| Deprecation (Stable) | 2/3 majority | 5 members |
| Breaking change (Stable) | 3/4 majority | 5 members |
| Deprecation (Provisional) | Simple majority | 3 members |
| Emergency rollback | Program Lead unilateral | 1 member |

---

## 6. Version Policy

### 6.1 Semver Rules

| Version Component | Incremented When |
|------------------|-----------------|
| Major (X.0.0) | Breaking changes, API removals |
| Minor (0.X.0) | New features, deprecations, non-breaking additions |
| Patch (0.0.X) | Bug fixes, security patches, documentation |

### 6.2 Pre-release Identifiers

| Identifier | Meaning | Example |
|-----------|---------|---------|
| `-alpha.N` | Experimental; no stability | 1.0.0-alpha.3 |
| `-beta.N` | Stabilizing; semver-soft | 1.0.0-beta.2 |
| `-rc.N` | Release candidate; semver-hard | 1.0.0-rc.1 |

### 6.3 Support Policy

| Release Type | Support Duration | Security Patches |
|-------------|-----------------|-----------------|
| Current major | Until next major + 6 months | Yes |
| Previous major (LTS) | 18 months from next major | Yes |
| Older majors | Community only | Critical only |

---

## 7. Ecosystem Compatibility

### 7.1 Minimum Supported Rust Version (MSRV)

| Channel | MSRV Policy |
|---------|-------------|
| Alpha | Latest stable Rust |
| Beta | Latest stable - 2 releases |
| GA | Latest stable - 4 releases (6-month window) |

### 7.2 Third-Party Crate Compatibility

Per the interop target ranking (T7.1):

| Tier | Crates | Compatibility Commitment |
|------|--------|------------------------|
| Critical | reqwest, axum, tonic | Full test coverage; breakage = SEV-1 |
| High | tower, hyper, deadpool | Integration tests; breakage = SEV-2 |
| Medium | rdkafka, redis, sqlx | Adapter tests; breakage = SEV-3 |
| Low | Remaining ecosystem | Best-effort |

---

## 8. Quality Gates

| Gate ID | Name | Condition | Evidence |
|---------|------|-----------|----------|
| CG-01 | Stability tiers defined | Stable/Provisional/Experimental/Internal | This document §2.1 |
| CG-02 | Compatibility dimensions defined | CD-01..CD-06 with enforcement | This document §2.2 |
| CG-03 | Deprecation lifecycle complete | PROPOSAL→REMOVED with requirements | This document §3 |
| CG-04 | Breaking change classification | BC-01..BC-06 with approval matrix | This document §4.1 |
| CG-05 | Governance board defined | Composition and decision thresholds | This document §5 |
| CG-06 | Version policy explicit | Semver rules and support duration | This document §6 |
| CG-07 | MSRV policy defined | Per-channel MSRV commitments | This document §7.1 |
| CG-08 | Ecosystem compatibility tiered | Critical/High/Medium/Low tiers | This document §7.2 |

---

## 9. Evidence Links

| Artifact | Reference |
|----------|-----------|
| Release channels | `docs/tokio_release_channels_stabilization_policy.md` |
| Migration lab KPI contract | `docs/tokio_migration_lab_kpi_contract.md` |
| Compatibility matrix | `docs/tokio_compatibility_limitation_matrix.md` |
| Interop target ranking | `docs/tokio_interop_target_ranking.md` |
| Migration cookbooks | `docs/tokio_migration_cookbooks.md` |
| Replacement roadmap | `docs/tokio_replacement_roadmap.md` |

---

## 10. CI Integration

Validation:
```bash
cargo test --test tokio_compatibility_governance_enforcement
rch exec 'cargo test --test tokio_compatibility_governance_enforcement'
```

---

## Appendix A: Cross-References

| Bead | Relationship | Description |
|------|-------------|-------------|
| `asupersync-2oh2u.11.10` | Prerequisite | Migration lab KPIs |
| `asupersync-2oh2u.11.5` | Prerequisite | Release channels |
| `asupersync-2oh2u.11.3` | Prerequisite | Compatibility matrix |
| `asupersync-2oh2u.11.8` | Downstream | Replacement claim RFC |
