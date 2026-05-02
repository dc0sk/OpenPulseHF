use anyhow::{Context, Result};
use reqwest::blocking::Client as HttpClient;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use openpulse_core::trust::{classify_connection_trust, CertificateSource, PublicKeyTrustLevel};

use crate::state::key_trust_from_publication_state;

// ── Response types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct IdentityRecord {
    pub record_id: String,
    pub station_id: String,
    pub callsign: String,
    pub publication_state: String,
    pub current_revision_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RevocationRecord {
    pub revocation_id: String,
    pub record_id: String,
    pub reason_code: String,
    pub effective_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrustBundleRecord {
    pub schema_version: String,
    pub bundle_id: String,
    pub records: Value,
}

// ── PKI client ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PkiClient {
    pub base_url: String,
    http: HttpClient,
}

impl PkiClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: HttpClient::new(),
        }
    }

    pub fn lookup_identity(&self, station_or_record_id: &str) -> Result<Option<IdentityRecord>> {
        let by_id_url = format!(
            "{}/api/v1/identities/{}",
            self.base_url, station_or_record_id
        );
        let by_id = self
            .http
            .get(by_id_url)
            .send()
            .context("pki request failed")?;
        if by_id.status().is_success() {
            return Ok(Some(by_id.json().context("invalid identity payload")?));
        }
        if by_id.status() != StatusCode::NOT_FOUND {
            anyhow::bail!("pki lookup failed: HTTP {}", by_id.status());
        }

        let by_station_url = format!("{}/api/v1/identities:lookup", self.base_url);
        let by_station = self
            .http
            .get(&by_station_url)
            .query(&[("station_id", station_or_record_id), ("limit", "1")])
            .send()
            .context("pki lookup request failed")?;
        if by_station.status().is_success() {
            let mut rows: Vec<IdentityRecord> =
                by_station.json().context("invalid identity list payload")?;
            if let Some(row) = rows.pop() {
                return Ok(Some(row));
            }
        } else {
            anyhow::bail!("pki station lookup failed: HTTP {}", by_station.status());
        }

        let by_callsign = self
            .http
            .get(by_station_url)
            .query(&[("callsign", station_or_record_id), ("limit", "1")])
            .send()
            .context("pki callsign lookup request failed")?;
        if !by_callsign.status().is_success() {
            anyhow::bail!("pki callsign lookup failed: HTTP {}", by_callsign.status());
        }

        let mut rows: Vec<IdentityRecord> = by_callsign
            .json()
            .context("invalid callsign lookup payload")?;
        Ok(rows.pop())
    }

    pub fn list_revocations(&self, record_id: &str) -> Result<Vec<RevocationRecord>> {
        let url = format!("{}/api/v1/revocations", self.base_url);
        let resp = self
            .http
            .get(url)
            .query(&[("record_id", record_id), ("limit", "50")])
            .send()
            .context("pki revocation query failed")?;
        if !resp.status().is_success() {
            anyhow::bail!("pki revocation lookup failed: HTTP {}", resp.status());
        }
        resp.json().context("invalid revocation payload")
    }

    pub fn get_current_bundle(&self) -> Result<Option<TrustBundleRecord>> {
        let url = format!("{}/api/v1/trust-bundles/current", self.base_url);
        let resp = self
            .http
            .get(url)
            .send()
            .context("pki trust-bundle request failed")?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            anyhow::bail!("pki trust-bundle query failed: HTTP {}", resp.status());
        }
        Ok(Some(resp.json().context("invalid trust bundle payload")?))
    }

    pub fn healthz(&self) -> Result<()> {
        let url = format!("{}/healthz", self.base_url);
        let resp = self
            .http
            .get(url)
            .send()
            .context("pki health request failed")?;
        if !resp.status().is_success() {
            anyhow::bail!("pki health check failed: HTTP {}", resp.status());
        }
        Ok(())
    }

    pub fn create_session_audit_event(&self, payload: &Value) -> Result<()> {
        let url = format!("{}/api/v1/session-audit-events", self.base_url);
        let resp = self
            .http
            .post(url)
            .json(payload)
            .send()
            .context("pki session audit request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let detail = resp
                .text()
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            anyhow::bail!("pki session audit insert failed: HTTP {status}: {detail}");
        }

        Ok(())
    }
}

// ── PKI helpers ───────────────────────────────────────────────────────────────

pub fn fetch_pki_trust(
    station_or_record_id: String,
    pki: &PkiClient,
) -> Result<Option<(openpulse_core::trust::TrustDecision, Value)>> {
    let Some(identity) = pki.lookup_identity(&station_or_record_id)? else {
        return Ok(None);
    };

    let revocations = pki.list_revocations(&identity.record_id)?;
    if !revocations.is_empty() {
        let decision = classify_connection_trust(
            PublicKeyTrustLevel::Untrusted,
            CertificateSource::OutOfBand,
            false,
        );
        return Ok(Some((
            decision,
            json!({
                "peer": station_or_record_id,
                "record_id": identity.record_id,
                "station_id": identity.station_id,
                "callsign": identity.callsign,
                "publication_state": identity.publication_state,
                "effective_revocation_state": "revoked",
                "revocation_count": revocations.len(),
                "revocations": revocations,
                "evidence_classes": ["operator", "gpg", "tqsl", "replication"],
            }),
        )));
    }

    let key_trust = key_trust_from_publication_state(&identity.publication_state);
    let decision = classify_connection_trust(key_trust, CertificateSource::OutOfBand, false);
    Ok(Some((
        decision,
        json!({
            "peer": station_or_record_id,
            "record_id": identity.record_id,
            "station_id": identity.station_id,
            "callsign": identity.callsign,
            "publication_state": identity.publication_state,
            "current_revision_id": identity.current_revision_id,
            "effective_revocation_state": "none",
            "revocation_count": 0,
            "evidence_classes": ["operator", "gpg", "tqsl", "replication"],
        }),
    )))
}

pub fn parse_public_key_trust_level(value: &str) -> Result<PublicKeyTrustLevel> {
    match value.to_lowercase().as_str() {
        "full" => Ok(PublicKeyTrustLevel::Full),
        "marginal" => Ok(PublicKeyTrustLevel::Marginal),
        "unknown" => Ok(PublicKeyTrustLevel::Unknown),
        "untrusted" => Ok(PublicKeyTrustLevel::Untrusted),
        "revoked" => Ok(PublicKeyTrustLevel::Revoked),
        _ => anyhow::bail!("invalid trust level"),
    }
}

pub fn parse_certificate_source(value: &str) -> Result<CertificateSource> {
    match value.to_lowercase().as_str() {
        "out_of_band" | "out-of-band" => Ok(CertificateSource::OutOfBand),
        "over_air" | "over-air" => Ok(CertificateSource::OverAir),
        _ => anyhow::bail!("invalid certificate source"),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_and_source_parsers_accept_expected_values() {
        assert_eq!(
            parse_public_key_trust_level("full").expect("trust"),
            PublicKeyTrustLevel::Full
        );
        assert_eq!(
            parse_public_key_trust_level("revoked").expect("trust"),
            PublicKeyTrustLevel::Revoked
        );
        assert!(parse_public_key_trust_level("bogus").is_err());

        assert_eq!(
            parse_certificate_source("out_of_band").expect("source"),
            CertificateSource::OutOfBand
        );
        assert_eq!(
            parse_certificate_source("over-air").expect("source"),
            CertificateSource::OverAir
        );
        assert!(parse_certificate_source("bogus").is_err());
    }
}
