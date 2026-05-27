use base64::{engine::general_purpose::STANDARD, Engine};
use ed25519_dalek::SigningKey;
use pki_tooling::startup_env::{optional_env, required_env, EnvVarError};
use pki_tooling::{build_router, AppState};
use sqlx::postgres::PgPoolOptions;
use std::{error::Error as _, net::SocketAddr};
use thiserror::Error;

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8080";

#[derive(Debug, Error)]
enum StartupError {
    #[error("{0} env var must be set")]
    MissingEnv(&'static str),
    #[error("{0} must not be empty")]
    EmptyEnv(&'static str),
    #[error("{0} must contain valid Unicode")]
    InvalidUnicode(&'static str),
    #[error("PKI_SIGNING_KEY env var must be set unless PKI_ALLOW_EPHEMERAL_KEY=true")]
    MissingSigningKey,
    #[error("PKI_SIGNING_KEY must be valid base64")]
    InvalidSigningKeyEncoding(#[source] base64::DecodeError),
    #[error("PKI_SIGNING_KEY seed must be exactly 32 bytes")]
    InvalidSigningKeyLength,
    #[error(
        "PKI_ALLOW_EPHEMERAL_KEY value '{value}' is invalid; expected one of: true, false, 1, 0, yes, no"
    )]
    InvalidEphemeralFlag { value: String },
    #[error("PKI_BIND_ADDR must be a valid socket address")]
    InvalidBindAddr(#[source] std::net::AddrParseError),
    #[error("failed to connect to database")]
    Database(#[source] sqlx::Error),
    #[error("failed to bind listener on {bind_addr}")]
    BindListener {
        bind_addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("server runtime failure")]
    Serve(#[source] std::io::Error),
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("fatal: {err}");
        eprintln!("fatal(debug): {err:?}");
        let mut source = err.source();
        while let Some(cause) = source {
            eprintln!("caused by: {cause}");
            source = cause.source();
        }
        std::process::exit(1);
    }
}

async fn run() -> Result<(), StartupError> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url = required_env("DATABASE_URL").map_err(map_env_error)?;
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .map_err(StartupError::Database)?;

    let signing_key = load_signing_key(
        optional_env("PKI_SIGNING_KEY").map_err(map_env_error)?,
        ephemeral_mode_enabled()?,
    )?;

    let api_key = required_env("PKI_API_KEY").map_err(map_env_error)?;
    let bind_addr = resolve_bind_addr(optional_env("PKI_BIND_ADDR").map_err(map_env_error)?)?;

    let state = AppState {
        db,
        signing_key,
        api_key,
    };

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .map_err(|source| StartupError::BindListener {
            bind_addr: bind_addr.to_string(),
            source,
        })?;

    tracing::info!("pki-tooling API listening on http://{bind_addr}");
    axum::serve(listener, app)
        .await
        .map_err(StartupError::Serve)
}

fn map_env_error(err: EnvVarError) -> StartupError {
    match err {
        EnvVarError::Missing(name) => StartupError::MissingEnv(name),
        EnvVarError::Empty(name) => StartupError::EmptyEnv(name),
        EnvVarError::InvalidUnicode(name) => StartupError::InvalidUnicode(name),
    }
}

fn ephemeral_mode_enabled() -> Result<bool, StartupError> {
    match optional_env("PKI_ALLOW_EPHEMERAL_KEY").map_err(map_env_error)? {
        Some(value) => parse_bool_flag(&value),
        None => Ok(false),
    }
}

fn parse_bool_flag(value: &str) -> Result<bool, StartupError> {
    if value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("1")
        || value.eq_ignore_ascii_case("yes")
    {
        return Ok(true);
    }

    if value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("0")
        || value.eq_ignore_ascii_case("no")
    {
        return Ok(false);
    }

    Err(StartupError::InvalidEphemeralFlag {
        value: value.to_string(),
    })
}

fn load_signing_key(
    signing_key_b64: Option<String>,
    allow_ephemeral: bool,
) -> Result<SigningKey, StartupError> {
    match signing_key_b64 {
        Some(b64) => {
            let seed = STANDARD
                .decode(&b64)
                .map_err(StartupError::InvalidSigningKeyEncoding)?;
            let arr: [u8; 32] = seed
                .as_slice()
                .try_into()
                .map_err(|_| StartupError::InvalidSigningKeyLength)?;
            Ok(SigningKey::from_bytes(&arr))
        }
        None if allow_ephemeral => {
            tracing::warn!(
                "PKI_SIGNING_KEY not set; using ephemeral signing key because PKI_ALLOW_EPHEMERAL_KEY=true"
            );
            use rand::RngCore;
            let mut seed = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut seed);
            Ok(SigningKey::from_bytes(&seed))
        }
        None => Err(StartupError::MissingSigningKey),
    }
}

fn resolve_bind_addr(bind_addr: Option<String>) -> Result<SocketAddr, StartupError> {
    bind_addr
        .unwrap_or_else(|| DEFAULT_BIND_ADDR.to_owned())
        .parse()
        .map_err(StartupError::InvalidBindAddr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signing_key_is_required_by_default() {
        let err = load_signing_key(None, false).unwrap_err();
        assert!(matches!(err, StartupError::MissingSigningKey));
    }

    #[test]
    fn explicit_ephemeral_mode_allows_generated_key() {
        assert!(load_signing_key(None, true).is_ok());
    }

    #[test]
    fn invalid_signing_key_length_is_rejected() {
        let err = load_signing_key(Some(STANDARD.encode([7u8; 31])), false).unwrap_err();
        assert!(matches!(err, StartupError::InvalidSigningKeyLength));
    }

    #[test]
    fn bool_flag_accepts_common_values() {
        assert!(parse_bool_flag("true").unwrap());
        assert!(parse_bool_flag("YES").unwrap());
        assert!(!parse_bool_flag("0").unwrap());
    }

    #[test]
    fn invalid_bool_flag_is_rejected() {
        let err = parse_bool_flag("sometimes").unwrap_err();
        assert!(matches!(err, StartupError::InvalidEphemeralFlag { .. }));
    }

    #[test]
    fn bind_addr_defaults_to_loopback() {
        assert_eq!(
            resolve_bind_addr(None).unwrap(),
            DEFAULT_BIND_ADDR.parse().unwrap()
        );
    }

    #[test]
    fn invalid_bind_addr_is_rejected() {
        let err = resolve_bind_addr(Some("not-an-address".to_owned())).unwrap_err();
        assert!(matches!(err, StartupError::InvalidBindAddr(_)));
    }
}
