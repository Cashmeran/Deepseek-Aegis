//! SecretProxy — MITM localhost HTTP proxy for API key injection.
//! Sandboxed processes only see placeholder keys (SANDBOX_PLACEHOLDER).
//! The proxy transparently replaces placeholders with real keys on outbound requests.

use std::collections::HashMap;

/// Maps placeholder tokens → real API keys
pub struct SecretProxy {
    key_map: HashMap<String, String>,
    listen_addr: String,
}

/// Access control for proxy
#[derive(Debug, Clone)]
pub struct ProxyAcl {
    pub allowed_hosts: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub require_sni_match: bool,
}

impl Default for ProxyAcl {
    fn default() -> Self {
        Self {
            allowed_hosts: vec!["api.deepseek.com".into()],
            allowed_paths: vec!["/anthropic/".into(), "/v1/".into()],
            require_sni_match: true,
        }
    }
}

impl SecretProxy {
    pub fn new(keys: HashMap<String, String>) -> Self {
        Self {
            key_map: keys,
            listen_addr: "127.0.0.1:9090".into(),
        }
    }

    /// Replace placeholder with real key in a header value
    pub fn replace_placeholder(&self, header_value: &str) -> String {
        let mut result = header_value.to_string();
        for (placeholder, real_key) in &self.key_map {
            result = result.replace(placeholder, real_key);
        }
        result
    }

    /// Check if a target URL is allowed by ACL
    pub fn check_acl(url: &str, acl: &ProxyAcl) -> bool {
        let parts: Vec<&str> = url.split('/').collect();
        if parts.len() < 3 { return false; }
        let host = parts[2];
        let path = format!("/{}", parts[3..].join("/"));

        if !acl.allowed_hosts.iter().any(|h| host.contains(h.as_str())) {
            return false;
        }
        if !acl.allowed_paths.iter().any(|p| path.starts_with(p.as_str())) {
            return false;
        }
        true
    }

    /// Build environment variables that route sandbox traffic through proxy
    pub fn env_vars(&self) -> Vec<(String, String)> {
        vec![
            ("HTTP_PROXY".into(), format!("http://{}", self.listen_addr)),
            ("HTTPS_PROXY".into(), format!("http://{}", self.listen_addr)),
            ("NO_PROXY".into(), "localhost,127.0.0.1".into()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_placeholder() {
        let mut keys = HashMap::new();
        keys.insert("SANDBOX_PLACEHOLDER".into(), "sk-real-key".into());
        let proxy = SecretProxy::new(keys);

        let result = proxy.replace_placeholder("Bearer SANDBOX_PLACEHOLDER");
        assert_eq!(result, "Bearer sk-real-key");
    }

    #[test]
    fn test_acl_allow_deepseek() {
        assert!(SecretProxy::check_acl(
            "https://api.deepseek.com/anthropic/v1/messages",
            &ProxyAcl::default()
        ));
    }

    #[test]
    fn test_acl_deny_unapproved_host() {
        assert!(!SecretProxy::check_acl(
            "https://evil.com/api",
            &ProxyAcl::default()
        ));
    }

    #[test]
    fn test_acl_deny_wrong_path() {
        assert!(!SecretProxy::check_acl(
            "https://api.deepseek.com/admin/keys",
            &ProxyAcl::default()
        ));
    }
}
