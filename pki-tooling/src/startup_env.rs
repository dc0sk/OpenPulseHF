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
