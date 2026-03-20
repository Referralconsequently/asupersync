//! Privacy-preserving export helpers for FABRIC metadata summaries.
//!
//! This module applies disclosure policy, subject blinding, and optional
//! differential-privacy-style noise to metadata that crosses a trust boundary.
//! Authoritative internal state always stays exact. Only exported summaries are
//! blinded or noised.

use super::ir::{MetadataDisclosure, PrivacyPolicy};
use crate::util::DetHasher;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::hash::{Hash, Hasher};
use thiserror::Error;

/// Exact internal metadata summary before any privacy transform is applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthoritativeMetadataSummary {
    /// Stable summary family or advisory name.
    pub summary_name: String,
    /// Internal tenant identifier.
    pub tenant: String,
    /// Internal subject or route key.
    pub subject: String,
    /// Exact message count before export noise.
    pub message_count: u64,
    /// Exact byte count before export noise.
    pub byte_count: u64,
    /// Exact error count before export noise.
    pub error_count: u64,
    /// Whether this export would cross a tenant boundary.
    pub cross_tenant: bool,
}

impl AuthoritativeMetadataSummary {
    fn validate(&self) -> Result<(), PrivacyExportError> {
        validate_text("summary_name", &self.summary_name)?;
        validate_text("tenant", &self.tenant)?;
        validate_text("subject", &self.subject)?;
        Ok(())
    }
}

/// Exported summary after policy-driven blinding and optional noise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportedMetadataSummary {
    /// Stable summary family or advisory name.
    pub summary_name: String,
    /// Policy name that governed the export.
    pub policy_name: String,
    /// Boundary disclosure mode used for the export.
    pub disclosure: MetadataDisclosure,
    /// Subject token shown to the observer after blinding.
    pub subject_token: String,
    /// Tenant token shown to the observer after blinding.
    pub tenant_token: String,
    /// Exported message count after optional noise.
    pub message_count: u64,
    /// Exported byte count after optional noise.
    pub byte_count: u64,
    /// Exported error count after optional noise.
    pub error_count: u64,
    /// Applied message-count noise delta.
    pub message_noise: i64,
    /// Applied byte-count noise delta.
    pub byte_noise: i64,
    /// Applied error-count noise delta.
    pub error_noise: i64,
    /// Budget spent by this export, when noise is enabled.
    pub privacy_budget_spent: Option<f64>,
    /// Whether the export crossed a tenant boundary.
    pub cross_tenant: bool,
}

/// Running budget for boundary-crossing privacy disclosures.
#[derive(Debug, Clone, PartialEq)]
pub struct PrivacyBudgetLedger {
    total_budget: f64,
    spent_budget: f64,
    disclosures: u64,
}

impl PrivacyBudgetLedger {
    /// Create a new finite privacy budget ledger.
    pub fn new(total_budget: f64) -> Result<Self, PrivacyExportError> {
        if !total_budget.is_finite() || total_budget <= 0.0 {
            return Err(PrivacyExportError::InvalidBudget {
                field: "total_budget",
                value: total_budget,
            });
        }
        Ok(Self {
            total_budget,
            spent_budget: 0.0,
            disclosures: 0,
        })
    }

    /// Remaining export budget.
    #[must_use]
    pub fn remaining_budget(&self) -> f64 {
        (self.total_budget - self.spent_budget).max(0.0)
    }

    /// Total budget already spent.
    #[must_use]
    pub fn spent_budget(&self) -> f64 {
        self.spent_budget
    }

    /// Number of accepted disclosures.
    #[must_use]
    pub const fn disclosures(&self) -> u64 {
        self.disclosures
    }

    fn spend(&mut self, epsilon: f64) -> Result<(), PrivacyExportError> {
        if !epsilon.is_finite() || epsilon <= 0.0 {
            return Err(PrivacyExportError::InvalidBudget {
                field: "epsilon",
                value: epsilon,
            });
        }

        let remaining = self.remaining_budget();
        if epsilon > remaining {
            return Err(PrivacyExportError::BudgetExhausted {
                requested: epsilon,
                remaining,
            });
        }

        self.spent_budget += epsilon;
        self.disclosures += 1;
        Ok(())
    }
}

/// Export-time privacy failures.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PrivacyExportError {
    /// Required summary fields must be non-empty.
    #[error("privacy summary field `{field}` must not be empty")]
    EmptyField {
        /// Field that failed validation.
        field: &'static str,
    },
    /// Privacy budgets must be positive finite values.
    #[error("privacy budget `{field}` must be finite and greater than zero, got {value}")]
    InvalidBudget {
        /// Budget field being validated.
        field: &'static str,
        /// Invalid value that was supplied.
        value: f64,
    },
    /// Cross-tenant disclosure requires explicit policy opt-in.
    #[error("privacy policy `{policy_name}` does not permit cross-tenant metadata export")]
    CrossTenantFlowDisallowed {
        /// Policy that rejected the export.
        policy_name: String,
    },
    /// Privacy export budget was exhausted.
    #[error("privacy budget exhausted: requested {requested}, remaining {remaining}")]
    BudgetExhausted {
        /// Requested epsilon spend.
        requested: f64,
        /// Remaining epsilon before the failed spend.
        remaining: f64,
    },
}

/// Export one metadata summary across a trust boundary.
///
/// `disclosure_nonce` intentionally makes repeated exports deterministic for
/// tests and replay while still producing field-specific independent noise.
pub fn export_metadata_summary(
    policy: &PrivacyPolicy,
    ledger: &mut PrivacyBudgetLedger,
    summary: &AuthoritativeMetadataSummary,
    disclosure_nonce: u64,
) -> Result<ExportedMetadataSummary, PrivacyExportError> {
    summary.validate()?;
    validate_text("policy_name", &policy.name)?;

    if summary.cross_tenant && !policy.allow_cross_tenant_flow {
        return Err(PrivacyExportError::CrossTenantFlowDisallowed {
            policy_name: policy.name.clone(),
        });
    }

    let privacy_budget_spent = if let Some(epsilon) = policy.noise_budget {
        ledger.spend(epsilon)?;
        Some(epsilon)
    } else {
        None
    };

    let subject_token = blind_subject(
        policy.metadata_disclosure,
        &summary.subject,
        policy.redact_subject_literals,
    );
    let tenant_token = blind_identifier(policy.metadata_disclosure, &summary.tenant);

    let message_noise = laplace_noise(
        noise_seed(policy, summary, "message_count", disclosure_nonce),
        privacy_budget_spent,
    );
    let byte_noise = laplace_noise(
        noise_seed(policy, summary, "byte_count", disclosure_nonce),
        privacy_budget_spent,
    );
    let error_noise = laplace_noise(
        noise_seed(policy, summary, "error_count", disclosure_nonce),
        privacy_budget_spent,
    );

    Ok(ExportedMetadataSummary {
        summary_name: summary.summary_name.clone(),
        policy_name: policy.name.clone(),
        disclosure: policy.metadata_disclosure,
        subject_token,
        tenant_token,
        message_count: apply_noise(summary.message_count, message_noise),
        byte_count: apply_noise(summary.byte_count, byte_noise),
        error_count: apply_noise(summary.error_count, error_noise),
        message_noise,
        byte_noise,
        error_noise,
        privacy_budget_spent,
        cross_tenant: summary.cross_tenant,
    })
}

fn validate_text(field: &'static str, value: &str) -> Result<(), PrivacyExportError> {
    if value.trim().is_empty() {
        return Err(PrivacyExportError::EmptyField { field });
    }
    Ok(())
}

fn blind_subject(disclosure: MetadataDisclosure, subject: &str, redact_literals: bool) -> String {
    match disclosure {
        MetadataDisclosure::Full if redact_literals => subject
            .split('.')
            .map(|_| "*")
            .collect::<Vec<_>>()
            .join("."),
        MetadataDisclosure::Full => subject.to_owned(),
        MetadataDisclosure::Hashed => hash_token(subject),
        MetadataDisclosure::Redacted => "<redacted>".to_owned(),
    }
}

fn blind_identifier(disclosure: MetadataDisclosure, value: &str) -> String {
    match disclosure {
        MetadataDisclosure::Full => value.to_owned(),
        MetadataDisclosure::Hashed => hash_token(value),
        MetadataDisclosure::Redacted => "<redacted>".to_owned(),
    }
}

fn hash_token(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut token = String::with_capacity("sha256:".len() + digest.len() * 2);
    token.push_str("sha256:");
    for byte in digest {
        token.push(hex_nibble(byte >> 4));
        token.push(hex_nibble(byte & 0x0f));
    }
    token
}

fn hex_nibble(nibble: u8) -> char {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    char::from(HEX_DIGITS[usize::from(nibble & 0x0f)])
}

fn noise_seed(
    policy: &PrivacyPolicy,
    summary: &AuthoritativeMetadataSummary,
    field: &str,
    disclosure_nonce: u64,
) -> u64 {
    let mut hasher = DetHasher::default();
    policy.name.hash(&mut hasher);
    summary.summary_name.hash(&mut hasher);
    summary.subject.hash(&mut hasher);
    summary.tenant.hash(&mut hasher);
    summary.cross_tenant.hash(&mut hasher);
    field.hash(&mut hasher);
    disclosure_nonce.hash(&mut hasher);
    hasher.finish()
}

fn laplace_noise(seed: u64, epsilon: Option<f64>) -> i64 {
    let Some(epsilon) = epsilon else {
        return 0;
    };

    let centered = unit_interval(seed) - 0.5;
    if centered == 0.0 {
        return 0;
    }

    let scale = 1.0 / epsilon;
    let noise = -scale * centered.signum() * (1.0 - 2.0 * centered.abs()).ln();
    noise.round() as i64
}

fn unit_interval(seed: u64) -> f64 {
    let bits = splitmix64(seed) >> 11;
    ((bits as f64) + 0.5) / ((1_u64 << 53) as f64)
}

fn splitmix64(mut state: u64) -> u64 {
    state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn apply_noise(value: u64, delta: i64) -> u64 {
    if delta >= 0 {
        value.saturating_add(delta as u64)
    } else {
        value.saturating_sub(delta.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary() -> AuthoritativeMetadataSummary {
        AuthoritativeMetadataSummary {
            summary_name: "fabric.advisory".to_owned(),
            tenant: "tenant-a".to_owned(),
            subject: "orders.eu.created".to_owned(),
            message_count: 41,
            byte_count: 4096,
            error_count: 2,
            cross_tenant: false,
        }
    }

    fn ledger() -> PrivacyBudgetLedger {
        PrivacyBudgetLedger::new(5.0).expect("valid privacy budget")
    }

    fn policy() -> PrivacyPolicy {
        PrivacyPolicy::default()
    }

    #[test]
    fn full_export_without_noise_preserves_authoritative_values() {
        let mut ledger = ledger();
        let exported = export_metadata_summary(&policy(), &mut ledger, &summary(), 7)
            .expect("full export should succeed");

        assert_eq!(exported.summary_name, "fabric.advisory");
        assert_eq!(exported.subject_token, "orders.eu.created");
        assert_eq!(exported.tenant_token, "tenant-a");
        assert_eq!(exported.message_count, 41);
        assert_eq!(exported.byte_count, 4096);
        assert_eq!(exported.error_count, 2);
        assert_eq!(exported.message_noise, 0);
        assert_eq!(exported.byte_noise, 0);
        assert_eq!(exported.error_noise, 0);
        assert_eq!(exported.privacy_budget_spent, None);
        assert_eq!(ledger.spent_budget(), 0.0);
    }

    #[test]
    fn hashed_export_blinds_subject_and_tenant() {
        let mut ledger = ledger();
        let mut policy = policy();
        policy.metadata_disclosure = MetadataDisclosure::Hashed;

        let exported = export_metadata_summary(&policy, &mut ledger, &summary(), 17)
            .expect("hashed export should succeed");

        assert!(exported.subject_token.starts_with("sha256:"));
        assert!(exported.tenant_token.starts_with("sha256:"));
        assert_ne!(exported.subject_token, "orders.eu.created");
        assert_ne!(exported.tenant_token, "tenant-a");
    }

    #[test]
    fn full_export_can_redact_subject_literals() {
        let mut ledger = ledger();
        let mut policy = policy();
        policy.redact_subject_literals = true;

        let exported = export_metadata_summary(&policy, &mut ledger, &summary(), 3)
            .expect("redacted full export should succeed");

        assert_eq!(exported.subject_token, "*.*.*");
        assert_eq!(exported.tenant_token, "tenant-a");
    }

    #[test]
    fn cross_tenant_export_requires_policy_opt_in() {
        let mut ledger = ledger();
        let mut summary = summary();
        summary.cross_tenant = true;

        let err = export_metadata_summary(&policy(), &mut ledger, &summary, 5)
            .expect_err("cross-tenant export should be rejected");

        assert!(matches!(
            err,
            PrivacyExportError::CrossTenantFlowDisallowed { .. }
        ));
    }

    #[test]
    fn privacy_budget_ledger_rejects_overspend() {
        let mut ledger = PrivacyBudgetLedger::new(0.75).expect("valid small budget");
        ledger.spend(0.5).expect("first spend fits");
        let err = ledger
            .spend(0.5)
            .expect_err("second spend should exceed budget");

        assert!(matches!(err, PrivacyExportError::BudgetExhausted { .. }));
        assert_eq!(ledger.disclosures(), 1);
    }

    #[test]
    fn noised_export_is_deterministic_and_preserves_authoritative_state() {
        let original = summary();
        let mut left_ledger = ledger();
        let mut right_ledger = ledger();
        let mut policy = policy();
        policy.noise_budget = Some(0.5);

        let left = export_metadata_summary(&policy, &mut left_ledger, &original, 99)
            .expect("left export should succeed");
        let right = export_metadata_summary(&policy, &mut right_ledger, &original, 99)
            .expect("right export should succeed");

        assert_eq!(left, right);
        assert_eq!(left.privacy_budget_spent, Some(0.5));
        assert_eq!(left_ledger.spent_budget(), 0.5);
        assert_eq!(left_ledger.disclosures(), 1);
        assert_eq!(original.message_count, 41);
        assert_eq!(original.byte_count, 4096);
        assert_eq!(original.error_count, 2);
    }

    #[test]
    fn invalid_summary_fields_fail_closed() {
        let mut ledger = ledger();
        let mut invalid = summary();
        invalid.subject = "   ".to_owned();

        let err = export_metadata_summary(&policy(), &mut ledger, &invalid, 11)
            .expect_err("invalid subject should fail");

        assert_eq!(err, PrivacyExportError::EmptyField { field: "subject" });
    }
}
