use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::{Host, Url};

#[derive(Debug, Clone, Default)]
pub struct NetworkGuard {
    allowed_domains: Vec<String>,
}

impl NetworkGuard {
    pub fn with_allowed_domains(domains: Vec<String>) -> Self {
        Self {
            allowed_domains: domains,
        }
    }

    /// Returns the list of explicitly allowed domains (empty = no restriction).
    pub fn allowed_domains(&self) -> Vec<String> {
        self.allowed_domains.clone()
    }

    pub fn check_url(&self, raw: &str) -> Result<Url, String> {
        let url = Url::parse(raw).map_err(|e| format!("Invalid URL: {e}"))?;
        if url.scheme() != "http" && url.scheme() != "https" {
            return Err(format!("Blocked scheme: {}", url.scheme()));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("URLs with embedded credentials are blocked".to_string());
        }
        match url.host() {
            None => return Err("URL has no host".to_string()),
            Some(Host::Ipv4(v4)) => self.check_ipv4(v4)?,
            Some(Host::Ipv6(v6)) => self.check_ipv6(v6)?,
            Some(Host::Domain(domain)) => {
                self.check_domain_name(domain)?;
                if !self.allowed_domains.is_empty() {
                    self.check_domain_allowed(domain)?;
                }
            }
        }
        Ok(url)
    }

    /// Returns Chrome-compatible URL glob patterns for all private/loopback IP ranges.
    /// Used with CDP `Network.setBlockedURLs` to block Chrome from requesting these
    /// hosts, including as redirect destinations.
    ///
    /// All patterns include a `/*` path suffix — Chrome's pattern engine requires
    /// a path component to match reliably (bare `*://10.*` may not match `http://10.0.0.1/path`).
    pub fn blocked_url_patterns() -> Vec<String> {
        let mut patterns = vec![
            // Loopback / unspecified
            "*://127.*/*".to_string(),
            "*://0.0.0.0/*".to_string(),
            "*://[::1]/*".to_string(),
            "*://[::1]:*/*".to_string(),
            // Localhost by name (all variants NetworkGuard blocks)
            "*://localhost/*".to_string(),
            "*://localhost:*/*".to_string(),
            "*://*.localhost/*".to_string(),
            "*://*.localhost:*/*".to_string(),
            "*://localhost.localdomain/*".to_string(),
            // 10.0.0.0/8
            "*://10.*/*".to_string(),
            // 192.168.0.0/16
            "*://192.168.*/*".to_string(),
            // 169.254.0.0/16 (link-local / AWS instance metadata)
            "*://169.254.*/*".to_string(),
            // Google Cloud metadata endpoint
            "*://metadata.google.internal/*".to_string(),
        ];
        // 172.16.0.0/12 (172.16.0.0 – 172.31.255.255)
        for i in 16u8..=31u8 {
            patterns.push(format!("*://172.{}.*/*", i));
        }
        patterns
    }

    fn check_domain_name(&self, host: &str) -> Result<(), String> {
        let lower = host.to_lowercase();
        if lower == "localhost"
            || lower == "localhost.localdomain"
            || lower.ends_with(".localhost")
            || lower == "metadata.google.internal"
        {
            return Err(format!("Blocked host: {host}"));
        }
        Ok(())
    }

    fn check_ipv4(&self, v4: Ipv4Addr) -> Result<(), String> {
        let ip = IpAddr::V4(v4);
        if ip.is_loopback() || ip.is_unspecified() {
            return Err(format!("Blocked loopback/unspecified IP: {v4}"));
        }
        let octets = v4.octets();
        if octets[0] == 10 {
            return Err(format!("Blocked private IP: {v4}"));
        }
        if octets[0] == 172 && (16..=31).contains(&octets[1]) {
            return Err(format!("Blocked private IP: {v4}"));
        }
        if octets[0] == 192 && octets[1] == 168 {
            return Err(format!("Blocked private IP: {v4}"));
        }
        if octets[0] == 169 && octets[1] == 254 {
            return Err(format!("Blocked link-local IP: {v4}"));
        }
        Ok(())
    }

    fn check_ipv6(&self, v6: Ipv6Addr) -> Result<(), String> {
        let ip = IpAddr::V6(v6);
        if ip.is_loopback() || ip.is_unspecified() {
            return Err(format!("Blocked loopback/unspecified IP: {v6}"));
        }
        Ok(())
    }

    fn check_domain_allowed(&self, host: &str) -> Result<(), String> {
        let lower = host.to_lowercase();
        for allowed in &self.allowed_domains {
            let allowed_lower = allowed.to_lowercase();
            if lower == allowed_lower || lower.ends_with(&format!(".{allowed_lower}")) {
                return Ok(());
            }
        }
        Err(format!(
            "Domain '{host}' not in allowed list: {:?}",
            self.allowed_domains
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_normal_https_urls() {
        let guard = NetworkGuard::default();
        assert!(guard.check_url("https://github.com/login").is_ok());
        assert!(guard.check_url("https://example.com/page?q=test").is_ok());
    }

    #[test]
    fn blocks_localhost() {
        let guard = NetworkGuard::default();
        assert!(guard.check_url("http://localhost:3000").is_err());
        assert!(guard.check_url("http://127.0.0.1:8080").is_err());
        assert!(guard.check_url("http://[::1]/admin").is_err());
        assert!(guard.check_url("http://0.0.0.0/").is_err());
    }

    #[test]
    fn blocks_private_ips() {
        let guard = NetworkGuard::default();
        assert!(guard.check_url("http://10.0.0.1/internal").is_err());
        assert!(guard.check_url("http://172.16.0.1/").is_err());
        assert!(guard.check_url("http://192.168.1.1/").is_err());
        assert!(guard.check_url("http://169.254.169.254/metadata").is_err());
    }

    #[test]
    fn blocks_non_http_schemes() {
        let guard = NetworkGuard::default();
        assert!(guard.check_url("file:///etc/passwd").is_err());
        assert!(guard.check_url("ftp://example.com").is_err());
    }

    #[test]
    fn blocks_urls_with_credentials() {
        let guard = NetworkGuard::default();
        assert!(guard.check_url("https://user:pass@evil.com/").is_err());
    }

    #[test]
    fn respects_domain_allowlist() {
        let guard = NetworkGuard::with_allowed_domains(vec!["github.com".to_string()]);
        assert!(guard.check_url("https://github.com/login").is_ok());
        assert!(guard.check_url("https://evil.com/phish").is_err());
        assert!(guard.check_url("https://sub.github.com/page").is_ok());
    }

    #[test]
    fn allowlist_empty_means_allow_all() {
        let guard = NetworkGuard::default();
        assert!(guard.check_url("https://anything.com").is_ok());
    }

    #[test]
    fn blocked_url_patterns_covers_all_private_ranges() {
        let patterns = NetworkGuard::blocked_url_patterns();
        // Should have at least: loopback, 10.*, 192.168.*, 169.254.*, 16 entries for 172.16-31.*
        assert!(
            patterns.len() >= 20,
            "expected ≥20 patterns, got {}",
            patterns.len()
        );
        // Must include the 172.16-31 range (with /* path suffix)
        for i in 16u8..=31u8 {
            let pat = format!("*://172.{}.*/*", i);
            assert!(patterns.contains(&pat), "missing pattern: {pat}");
        }
        // Must include loopback and link-local
        assert!(patterns.iter().any(|p| p.contains("127.")));
        assert!(patterns.iter().any(|p| p.contains("169.254.")));
        assert!(patterns.iter().any(|p| p.contains("localhost")));
    }
}
