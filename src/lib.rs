pub mod processor;
pub mod signing;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignetError {
    #[error("invalid covered component: {0}")]
    InvalidComponent(String),

    #[error("signing failed: {0}")]
    SigningFailed(String),
}

pub struct SignetConfig {
    pub port: u16,
    pub key_path: String,
}

impl SignetConfig {
    pub fn from_env() -> Self {
        let port = std::env::var("SIGNET_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(50051);
        let key_path =
            std::env::var("SIGNET_KEY_PATH").unwrap_or_else(|_| "key.pem".to_string());
        Self { port, key_path }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Env var tests need serialization since env vars are global state
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // SAFETY: env var tests are serialized via ENV_LOCK so no concurrent mutation
    unsafe fn clear_signet_env() {
        unsafe {
            std::env::remove_var("SIGNET_PORT");
            std::env::remove_var("SIGNET_KEY_PATH");
        }
    }

    #[test]
    fn default_port_is_50051() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe { clear_signet_env() };
        let config = SignetConfig::from_env();
        assert_eq!(config.port, 50051);
        assert_eq!(config.key_path, "key.pem");
    }

    #[test]
    fn env_var_overrides_port() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("SIGNET_PORT", "8080");
        }
        let config = SignetConfig::from_env();
        unsafe { clear_signet_env() };
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn env_var_overrides_key_path() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("SIGNET_KEY_PATH", "/secrets/signing.pem");
        }
        let config = SignetConfig::from_env();
        unsafe { clear_signet_env() };
        assert_eq!(config.key_path, "/secrets/signing.pem");
    }

    #[test]
    fn invalid_port_falls_back_to_default() {
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("SIGNET_PORT", "not-a-number");
        }
        let config = SignetConfig::from_env();
        unsafe { clear_signet_env() };
        assert_eq!(config.port, 50051);
    }
}
