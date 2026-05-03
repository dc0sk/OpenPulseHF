use ed25519_dalek::{
    Signature as EdSig, Signer as EdSigner, SigningKey as EdSigningKey, Verifier as EdVerifier,
    VerifyingKey as EdVerifyingKey,
};
use ml_dsa::{
    signature::{Keypair as MlDsaKeypair, Signer as MlSigner, Verifier as MlVerifier},
    EncodedVerifyingKey, KeyGen, MlDsa44,
};
use ml_kem::{
    kem::{Decapsulate, Encapsulate, KeyExport},
    Ciphertext, DecapsulationKey, EncapsulationKey, Key, MlKem768, Seed as MlKemSeed,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::handshake::TrustStore;
use crate::trust::{
    evaluate_handshake, CertificateSource, HandshakeDecision, PolicyProfile, PublicKeyTrustLevel,
    SigningMode, TrustError,
};

// ------------------------------------------------------------------
// Size constants (verified against FIPS 203/204)
// ------------------------------------------------------------------

pub const ML_DSA_44_PUBKEY_SIZE: usize = 1312;
pub const ML_DSA_44_SIG_SIZE: usize = 2420;
pub const ML_KEM_768_EK_SIZE: usize = 1184;
/// ML-KEM-768 decapsulation key in d||z seed form (64 bytes).
pub const ML_KEM_768_DK_SIZE: usize = 64;
pub const ML_KEM_768_CT_SIZE: usize = 1088;
pub const ML_KEM_768_SS_SIZE: usize = 32;

// ------------------------------------------------------------------
// Error type
// ------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum PqHandshakeError {
    #[error("invalid ML-DSA-44 public key")]
    InvalidPublicKey,
    #[error("invalid ML-DSA-44 or Ed25519 signature")]
    InvalidSignature,
    #[error("invalid ML-KEM-768 ciphertext or key")]
    InvalidCiphertext,
    #[error("trust policy rejected: {0:?}")]
    TrustPolicyRejected(TrustError),
    #[error("session ID mismatch: expected {expected}, got {got}")]
    SessionIdMismatch { expected: String, got: String },
    #[error("serialization error: {0}")]
    SerializationError(String),
}

impl From<TrustError> for PqHandshakeError {
    fn from(e: TrustError) -> Self {
        PqHandshakeError::TrustPolicyRejected(e)
    }
}

// ------------------------------------------------------------------
// Wire frame body structs (canonical — excludes signature fields)
// ------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct PqConReqBody {
    station_id: String,
    classical_pubkey: Vec<u8>,
    pq_pubkey: Vec<u8>,
    kem_pubkey: Vec<u8>,
    signing_modes: Vec<SigningMode>,
    session_id: String,
}

#[derive(Serialize, Deserialize)]
struct PqConAckBody {
    station_id: String,
    classical_pubkey: Vec<u8>,
    pq_pubkey: Vec<u8>,
    kem_ciphertext: Vec<u8>,
    selected_mode: SigningMode,
    session_id: String,
}

// ------------------------------------------------------------------
// Wire frame structs
// ------------------------------------------------------------------

/// Post-quantum connection request (initiator → responder).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqConReq {
    pub station_id: String,
    /// Ed25519 verifying key (32 B).
    pub classical_pubkey: Vec<u8>,
    /// ML-DSA-44 verifying key (1312 B).
    pub pq_pubkey: Vec<u8>,
    /// ML-KEM-768 encapsulation key (1184 B).
    pub kem_pubkey: Vec<u8>,
    pub signing_modes: Vec<SigningMode>,
    pub session_id: String,
    /// Ed25519 signature (64 B); empty when mode is `Pq`-only.
    pub classical_signature: Vec<u8>,
    /// ML-DSA-44 signature (2420 B).
    pub pq_signature: Vec<u8>,
}

/// Post-quantum connection acknowledgment (responder → initiator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqConAck {
    pub station_id: String,
    /// Ed25519 verifying key (32 B).
    pub classical_pubkey: Vec<u8>,
    /// ML-DSA-44 verifying key (1312 B).
    pub pq_pubkey: Vec<u8>,
    /// ML-KEM-768 ciphertext (1088 B).
    pub kem_ciphertext: Vec<u8>,
    pub selected_mode: SigningMode,
    /// Must echo the session_id from the corresponding PqConReq.
    pub session_id: String,
    /// Ed25519 signature (64 B); empty when mode is `Pq`.
    pub classical_signature: Vec<u8>,
    /// ML-DSA-44 signature (2420 B).
    pub pq_signature: Vec<u8>,
}

// ------------------------------------------------------------------
// Key generation
// ------------------------------------------------------------------

/// Returns (signing_key_bytes [32 B seed], verifying_key_bytes [1312 B]).
pub fn generate_ml_dsa_44_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut seed_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut seed_bytes);
    let sk = MlDsa44::from_seed(&ml_dsa::Seed::from(seed_bytes));
    let vk_encoded = sk.verifying_key().encode();
    let vk_bytes: Vec<u8> = vk_encoded.to_vec();
    (seed_bytes.to_vec(), vk_bytes)
}

/// Returns (decapsulation_key_bytes [64 B seed], encapsulation_key_bytes [1184 B]).
pub fn generate_ml_kem_768_keypair() -> (Vec<u8>, Vec<u8>) {
    let mut seed_bytes = [0u8; 64];
    rand::rngs::OsRng.fill_bytes(&mut seed_bytes);
    let dk = DecapsulationKey::<MlKem768>::from_seed(MlKemSeed::from(seed_bytes));
    let ek_encoded = dk.encapsulation_key().to_bytes();
    let ek_bytes: Vec<u8> = ek_encoded.to_vec();
    let dk_seed = dk.to_seed().expect("freshly generated key has seed");
    let dk_bytes: Vec<u8> = dk_seed.to_vec();
    (dk_bytes, ek_bytes)
}

// ------------------------------------------------------------------
// Internal signing/verification helpers
// ------------------------------------------------------------------

fn ml_dsa_sign(signing_key_seed: &[u8], message: &[u8]) -> Result<Vec<u8>, PqHandshakeError> {
    let seed_arr: [u8; 32] = signing_key_seed
        .try_into()
        .map_err(|_| PqHandshakeError::InvalidPublicKey)?;
    let sk = MlDsa44::from_seed(&ml_dsa::Seed::from(seed_arr));
    let sig: ml_dsa::Signature<MlDsa44> = sk.sign(message);
    let sig_encoded = sig.encode();
    Ok(sig_encoded.to_vec())
}

fn ml_dsa_verify(
    vk_bytes: &[u8],
    message: &[u8],
    sig_bytes: &[u8],
) -> Result<(), PqHandshakeError> {
    let vk_arr = EncodedVerifyingKey::<MlDsa44>::try_from(vk_bytes)
        .map_err(|_| PqHandshakeError::InvalidPublicKey)?;
    let vk = ml_dsa::VerifyingKey::<MlDsa44>::decode(&vk_arr);
    let sig = ml_dsa::Signature::<MlDsa44>::try_from(sig_bytes)
        .map_err(|_| PqHandshakeError::InvalidSignature)?;
    vk.verify(message, &sig)
        .map_err(|_| PqHandshakeError::InvalidSignature)
}

fn ed25519_sign(classical_seed: &[u8; 32], message: &[u8]) -> Vec<u8> {
    let sk = EdSigningKey::from_bytes(classical_seed);
    let sig: EdSig = sk.sign(message);
    sig.to_bytes().to_vec()
}

fn ed25519_verify(vk_bytes: &[u8], message: &[u8], sig_bytes: &[u8]) -> bool {
    let Ok(arr): Result<[u8; 32], _> = vk_bytes.try_into() else {
        return false;
    };
    let Ok(sig_arr): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let Ok(vk) = EdVerifyingKey::from_bytes(&arr) else {
        return false;
    };
    let sig = EdSig::from_bytes(&sig_arr);
    vk.verify(message, &sig).is_ok()
}

fn cert_source_for_trust(trust_level: PublicKeyTrustLevel) -> CertificateSource {
    match trust_level {
        PublicKeyTrustLevel::Full => CertificateSource::OutOfBand,
        _ => CertificateSource::OverAir,
    }
}

/// Returns true if the given mode list represents Pq-only (no Hybrid).
fn is_pq_only(modes: &[SigningMode]) -> bool {
    modes.contains(&SigningMode::Pq) && !modes.contains(&SigningMode::Hybrid)
}

// ------------------------------------------------------------------
// Handshake creation
// ------------------------------------------------------------------

/// Build and sign a PqConReq.
///
/// Hybrid mode: signs with both Ed25519 and ML-DSA-44.
/// Pq-only mode: signs with ML-DSA-44 only; `classical_signature` is empty.
pub fn create_pq_conreq(
    station_id: &str,
    classical_seed: &[u8; 32],
    pq_signing_key: &[u8],
    kem_ek: &[u8],
    signing_modes: Vec<SigningMode>,
    session_id: &str,
) -> Result<PqConReq, PqHandshakeError> {
    let ed_sk = EdSigningKey::from_bytes(classical_seed);
    let classical_pubkey = ed_sk.verifying_key().to_bytes().to_vec();

    let seed_arr: [u8; 32] = pq_signing_key
        .try_into()
        .map_err(|_| PqHandshakeError::InvalidPublicKey)?;
    let mldsa_sk = MlDsa44::from_seed(&ml_dsa::Seed::from(seed_arr));
    let pq_pubkey_encoded = mldsa_sk.verifying_key().encode();
    let pq_pubkey: Vec<u8> = pq_pubkey_encoded.to_vec();

    let body = PqConReqBody {
        station_id: station_id.to_string(),
        classical_pubkey: classical_pubkey.clone(),
        pq_pubkey: pq_pubkey.clone(),
        kem_pubkey: kem_ek.to_vec(),
        signing_modes: signing_modes.clone(),
        session_id: session_id.to_string(),
    };
    let canonical = serde_json::to_vec(&body)
        .map_err(|e| PqHandshakeError::SerializationError(e.to_string()))?;

    let pq_signature = ml_dsa_sign(pq_signing_key, &canonical)?;
    let classical_signature = if is_pq_only(&signing_modes) {
        vec![]
    } else {
        ed25519_sign(classical_seed, &canonical)
    };

    Ok(PqConReq {
        station_id: station_id.to_string(),
        classical_pubkey,
        pq_pubkey,
        kem_pubkey: kem_ek.to_vec(),
        signing_modes,
        session_id: session_id.to_string(),
        classical_signature,
        pq_signature,
    })
}

/// Build and sign a PqConAck; encapsulates the KEM key from `req_kem_ek`.
///
/// Returns `(PqConAck, shared_secret_bytes [32 B])`.
pub fn create_pq_conack(
    station_id: &str,
    classical_seed: &[u8; 32],
    pq_signing_key: &[u8],
    req_kem_ek: &[u8],
    selected_mode: SigningMode,
    session_id: &str,
) -> Result<(PqConAck, Vec<u8>), PqHandshakeError> {
    let ed_sk = EdSigningKey::from_bytes(classical_seed);
    let classical_pubkey = ed_sk.verifying_key().to_bytes().to_vec();

    let seed_arr: [u8; 32] = pq_signing_key
        .try_into()
        .map_err(|_| PqHandshakeError::InvalidPublicKey)?;
    let mldsa_sk = MlDsa44::from_seed(&ml_dsa::Seed::from(seed_arr));
    let pq_pubkey_encoded = mldsa_sk.verifying_key().encode();
    let pq_pubkey: Vec<u8> = pq_pubkey_encoded.to_vec();

    // KEM encapsulation
    let ek_arr = Key::<EncapsulationKey<MlKem768>>::try_from(req_kem_ek)
        .map_err(|_| PqHandshakeError::InvalidPublicKey)?;
    let ek = EncapsulationKey::<MlKem768>::new(&ek_arr)
        .map_err(|_| PqHandshakeError::InvalidPublicKey)?;
    let (ct, ss) = ek.encapsulate();
    let kem_ciphertext: Vec<u8> = ct.to_vec();
    let shared_secret: Vec<u8> = ss.to_vec();

    let body = PqConAckBody {
        station_id: station_id.to_string(),
        classical_pubkey: classical_pubkey.clone(),
        pq_pubkey: pq_pubkey.clone(),
        kem_ciphertext: kem_ciphertext.clone(),
        selected_mode,
        session_id: session_id.to_string(),
    };
    let canonical = serde_json::to_vec(&body)
        .map_err(|e| PqHandshakeError::SerializationError(e.to_string()))?;

    let pq_signature = ml_dsa_sign(pq_signing_key, &canonical)?;
    let classical_signature = if selected_mode == SigningMode::Pq {
        vec![]
    } else {
        ed25519_sign(classical_seed, &canonical)
    };

    Ok((
        PqConAck {
            station_id: station_id.to_string(),
            classical_pubkey,
            pq_pubkey,
            kem_ciphertext,
            selected_mode,
            session_id: session_id.to_string(),
            classical_signature,
            pq_signature,
        },
        shared_secret,
    ))
}

/// Decapsulate the ML-KEM-768 ciphertext from `PqConAck` to recover the shared secret.
pub fn kem_decapsulate(dk: &[u8], ct: &[u8]) -> Result<Vec<u8>, PqHandshakeError> {
    let dk_seed: [u8; 64] = dk
        .try_into()
        .map_err(|_| PqHandshakeError::InvalidCiphertext)?;
    let kem_dk = DecapsulationKey::<MlKem768>::from_seed(MlKemSeed::from(dk_seed));
    let ct_arr =
        Ciphertext::<MlKem768>::try_from(ct).map_err(|_| PqHandshakeError::InvalidCiphertext)?;
    let ss = kem_dk.decapsulate(&ct_arr);
    Ok(ss.to_vec())
}

// ------------------------------------------------------------------
// Verification
// ------------------------------------------------------------------

fn canonical_req_bytes(req: &PqConReq) -> Result<Vec<u8>, PqHandshakeError> {
    let body = PqConReqBody {
        station_id: req.station_id.clone(),
        classical_pubkey: req.classical_pubkey.clone(),
        pq_pubkey: req.pq_pubkey.clone(),
        kem_pubkey: req.kem_pubkey.clone(),
        signing_modes: req.signing_modes.clone(),
        session_id: req.session_id.clone(),
    };
    serde_json::to_vec(&body).map_err(|e| PqHandshakeError::SerializationError(e.to_string()))
}

fn canonical_ack_bytes(ack: &PqConAck) -> Result<Vec<u8>, PqHandshakeError> {
    let body = PqConAckBody {
        station_id: ack.station_id.clone(),
        classical_pubkey: ack.classical_pubkey.clone(),
        pq_pubkey: ack.pq_pubkey.clone(),
        kem_ciphertext: ack.kem_ciphertext.clone(),
        selected_mode: ack.selected_mode,
        session_id: ack.session_id.clone(),
    };
    serde_json::to_vec(&body).map_err(|e| PqHandshakeError::SerializationError(e.to_string()))
}

/// Verify a received PqConReq and evaluate trust.
pub fn verify_pq_conreq(
    req: &PqConReq,
    trust_store: &dyn TrustStore,
    policy: PolicyProfile,
    local_min_mode: SigningMode,
) -> Result<HandshakeDecision, PqHandshakeError> {
    let canonical = canonical_req_bytes(req)?;

    // ML-DSA-44 signature always required
    ml_dsa_verify(&req.pq_pubkey, &canonical, &req.pq_signature)?;

    // Ed25519 signature required unless Pq-only
    if !is_pq_only(&req.signing_modes)
        && !ed25519_verify(&req.classical_pubkey, &canonical, &req.classical_signature)
    {
        return Err(PqHandshakeError::InvalidSignature);
    }

    let key_trust = trust_store.trust_level(&req.station_id);
    let cert_source = cert_source_for_trust(key_trust);
    let decision = evaluate_handshake(
        policy,
        local_min_mode,
        &req.signing_modes,
        key_trust,
        cert_source,
        false,
    )?;
    Ok(decision)
}

/// Verify a received PqConAck, checking session_id and signatures.
pub fn verify_pq_conack(
    ack: &PqConAck,
    req_session_id: &str,
    trust_store: &dyn TrustStore,
    policy: PolicyProfile,
    local_min_mode: SigningMode,
) -> Result<HandshakeDecision, PqHandshakeError> {
    if ack.session_id != req_session_id {
        return Err(PqHandshakeError::SessionIdMismatch {
            expected: req_session_id.to_string(),
            got: ack.session_id.clone(),
        });
    }

    let canonical = canonical_ack_bytes(ack)?;

    ml_dsa_verify(&ack.pq_pubkey, &canonical, &ack.pq_signature)?;

    if ack.selected_mode != SigningMode::Pq
        && !ed25519_verify(&ack.classical_pubkey, &canonical, &ack.classical_signature)
    {
        return Err(PqHandshakeError::InvalidSignature);
    }

    let key_trust = trust_store.trust_level(&ack.station_id);
    let cert_source = cert_source_for_trust(key_trust);
    let decision = evaluate_handshake(
        policy,
        local_min_mode,
        &[ack.selected_mode],
        key_trust,
        cert_source,
        false,
    )?;
    Ok(decision)
}

// ------------------------------------------------------------------
// SAR serialization helpers
// ------------------------------------------------------------------

/// Serialize PqConReq to bytes for SAR transport.
pub fn encode_pq_conreq(req: &PqConReq) -> Result<Vec<u8>, PqHandshakeError> {
    serde_json::to_vec(req).map_err(|e| PqHandshakeError::SerializationError(e.to_string()))
}

/// Deserialize PqConReq from bytes after SAR reassembly.
pub fn decode_pq_conreq(bytes: &[u8]) -> Result<PqConReq, PqHandshakeError> {
    serde_json::from_slice(bytes).map_err(|e| PqHandshakeError::SerializationError(e.to_string()))
}

/// Serialize PqConAck to bytes for SAR transport.
pub fn encode_pq_conack(ack: &PqConAck) -> Result<Vec<u8>, PqHandshakeError> {
    serde_json::to_vec(ack).map_err(|e| PqHandshakeError::SerializationError(e.to_string()))
}

/// Deserialize PqConAck from bytes after SAR reassembly.
pub fn decode_pq_conack(bytes: &[u8]) -> Result<PqConAck, PqHandshakeError> {
    serde_json::from_slice(bytes).map_err(|e| PqHandshakeError::SerializationError(e.to_string()))
}
