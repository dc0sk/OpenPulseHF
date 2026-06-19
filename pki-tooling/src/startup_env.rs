use std::env;

#[derive(Debug)]
pub enum EnvVarError {
    Missing(&'static str),
    Empty(&'static str),
    InvalidUnicode(&'static str),
}

pub fn required_env(name: &'static str) -> Result<String, EnvVarError> {
    optional_env(name)?.ok_or(EnvVarError::Missing(name))
}

pub fn optional_env(name: &'static str) -> Result<Option<String>, EnvVarError> {
    match env::var(name) {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Err(EnvVarError::Empty(name))
            } else {
                Ok(Some(trimmed.to_owned()))
            }
        }
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(EnvVarError::InvalidUnicode(name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restore_env(key: &str, previous: Option<String>) {
        match previous {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn required_env_reports_missing() {
        let missing = "OPENPULSE_TEST_REQUIRED_MISSING";
        let previous = std::env::var(missing).ok();
        std::env::remove_var(missing);
        let err = required_env(missing).unwrap_err();
        restore_env(missing, previous);
        assert!(matches!(
            err,
            EnvVarError::Missing("OPENPULSE_TEST_REQUIRED_MISSING")
        ));
    }

    #[test]
    fn optional_env_rejects_empty_values() {
        let key = "OPENPULSE_TEST_EMPTY";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "   ");
        let err = optional_env(key).unwrap_err();
        restore_env(key, previous);
        assert!(matches!(err, EnvVarError::Empty("OPENPULSE_TEST_EMPTY")));
    }

    #[test]
    fn required_env_trims_and_returns_value() {
        let key = "OPENPULSE_TEST_REQUIRED_VALUE";
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "  abc123  ");
        let value = required_env(key).unwrap();
        restore_env(key, previous);
        assert_eq!(value, "abc123");
    }
}
