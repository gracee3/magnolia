use anyhow::{Context, Result};
use ed25519_dalek::{Verifier, VerifyingKey, Signature};
use sha2::{Sha256, Digest};
use std::path::Path;
use log::{info, warn};

pub struct PluginVerifier {
    trusted_keys: Vec<VerifyingKey>,
}

impl PluginVerifier {
    pub fn new() -> Self {
        Self {
            trusted_keys: Self::load_trusted_keys(),
        }
    }
    
    fn load_trusted_keys() -> Vec<VerifyingKey> {
        let mut keys = Vec::new();
        
        // Load from ~/.talisman/trusted_keys.txt
        if let Some(home) = dirs::home_dir() {
            let key_file = home.join(".talisman/trusted_keys.txt");
            if let Ok(content) = std::fs::read_to_string(&key_file) {
                for (line_num, line) in content.lines().enumerate() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    
                    match hex::decode(line) {
                        Ok(bytes) => match VerifyingKey::from_bytes(&bytes.try_into().unwrap_or([0u8; 32])) {
                            Ok(key) => {
                                keys.push(key);
                            }
                            Err(e) => warn!("Invalid key at line {}: {}", line_num + 1, e),
                        },
                        Err(e) => warn!("Invalid hex at line {}: {}", line_num + 1, e),
                    }
                }
            } else {
                warn!("No trusted keys file found at {}", key_file.display());
            }
        }
        
        info!("Loaded {} trusted keys", keys.len());
        keys
    }
    
    /// Verify a plugin against trusted keys
    /// Expects a detached signature file at {plugin_path}.sig
    pub fn verify_plugin(&self, plugin_path: &Path) -> Result<bool> {
        if self.trusted_keys.is_empty() {
            warn!("No trusted keys configured - skipping verification");
            return Ok(false);
        }

        // Read plugin file
        let plugin_bytes = std::fs::read(plugin_path)
            .with_context(|| format!("Failed to read plugin: {}", plugin_path.display()))?;
        
        // Read signature file (.sig)
        let _sig_path = plugin_path.with_extension("so.sig"); // Assumes .so -> .so.sig
        // If extension was .dll, this replaces it with .sig. We want append or replace extension?
        // Typically .so.sig or just .sig. Let's try appending.
        let sig_path = if let Some(ext) = plugin_path.extension() {
            let mut p = plugin_path.to_path_buf();
            let mut ext_os = ext.to_os_string();
            ext_os.push(".sig");
            p.set_extension(ext_os);
            p
        } else {
            plugin_path.with_extension("sig")
        };
        
        if !sig_path.exists() {
            warn!("No signature file found: {}", sig_path.display());
            return Ok(false);
        }
        
        let sig_bytes = std::fs::read(&sig_path)
            .with_context(|| format!("Failed to read signature: {}", sig_path.display()))?;
            
        let signature_bytes: [u8; 64] = sig_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Invalid signature length"))?;
            
        let signature = Signature::from_bytes(&signature_bytes);
        
        // Hash plugin
        let mut hasher = Sha256::new();
        hasher.update(&plugin_bytes);
        // ed25519-dalek 2.0 verify expects the MESSAGE, not the HASH, unless using prehashed variant.
        // If we want to verify the file content, we should pass the content strictly if it fits in memory.
        // If the plugins are large, we should use `verify_prehashed` or similar.
        // Assuming for now we pass the bytes directly if small enough, or verify expects bytes.
        // Verifier trait: verify(&self, msg: &[u8], signature: &Signature)
        // If the signer signed the raw bytes, we pass raw bytes.
        // If signer signed HASH, we need to pass HASH.
        // Let's assume standard signing behavior (sign message).
        
        // Verify against any trusted key
        for key in &self.trusted_keys {
            if key.verify(&plugin_bytes, &signature).is_ok() {
                info!("Plugin verified with key: {}", hex::encode(key.as_bytes()));
                return Ok(true);
            }
        }
        
        warn!("Signature verification failed for {}", plugin_path.display());
        Ok(false)
    }
}
