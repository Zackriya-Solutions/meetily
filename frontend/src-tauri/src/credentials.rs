//! Secure credential storage using the platform keychain.
//! On macOS this uses the Keychain, on Windows the Credential Manager,
//! and on Linux the Secret Service (via libsecret).

use keyring::Entry;
use log::{info, warn};

const SERVICE_NAME: &str = "com.meetily.ai";

/// Store an API key in the platform keychain.
pub fn store_api_key(provider: &str, key: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, provider)
        .map_err(|e| format!("Failed to create keyring entry for {}: {}", provider, e))?;
    entry
        .set_password(key)
        .map_err(|e| format!("Failed to store API key for {}: {}", provider, e))?;
    info!("Stored API key for provider '{}' in keychain", provider);
    Ok(())
}

/// Retrieve an API key from the platform keychain.
pub fn get_api_key(provider: &str) -> Result<Option<String>, String> {
    let entry = Entry::new(SERVICE_NAME, provider)
        .map_err(|e| format!("Failed to create keyring entry for {}: {}", provider, e))?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to retrieve API key for {}: {}", provider, e)),
    }
}

/// Delete an API key from the platform keychain.
pub fn delete_api_key(provider: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, provider)
        .map_err(|e| format!("Failed to create keyring entry for {}: {}", provider, e))?;
    match entry.delete_credential() {
        Ok(()) => {
            info!("Deleted API key for provider '{}' from keychain", provider);
            Ok(())
        }
        Err(keyring::Error::NoEntry) => {
            warn!(
                "No API key found for provider '{}' in keychain (already deleted?)",
                provider
            );
            Ok(())
        }
        Err(e) => Err(format!(
            "Failed to delete API key for {}: {}",
            provider, e
        )),
    }
}

/// Migrate a plaintext API key from the database to the keychain.
/// Stores the key in the keychain and returns true if a key was migrated.
pub fn migrate_key_to_keychain(provider: &str, plaintext_key: Option<&str>) -> Result<bool, String> {
    match plaintext_key {
        Some(key) if !key.is_empty() => {
            store_api_key(provider, key)?;
            info!(
                "Migrated API key for '{}' from database to keychain",
                provider
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}
