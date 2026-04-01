// src/anonymizer/entropy.rs
//
// Entropy-based heuristic for detecting high-entropy strings near credential context words.
// This is a catch-all for credential formats not covered by known Tier 1 patterns.
//
// A hit requires BOTH:
//   1. Shannon entropy ≥ 3.5 bits/char on a token of ≥ 20 chars
//   2. A context word within 100 chars before the token (case-insensitive)
//
// The context requirement is the critical false-positive filter. A random base64 string in
// plain prose does not trigger this; a random base64 string after "access_token:" does.

/// A candidate string that looks like it could be an undetected credential.
#[derive(Debug, Clone, PartialEq)]
pub struct EntropyHit {
    /// Byte offset of the start of the suspicious token in the original text.
    pub start: usize,
    /// Byte offset of the end (exclusive).
    pub end: usize,
    /// Shannon entropy in bits/char.
    pub entropy: f64,
    /// The context keyword that was found nearby (lowercased).
    pub context_word: &'static str,
}

/// Minimum token length to consider.
const MIN_LEN: usize = 20;
/// Entropy threshold in bits/char. Random base64 ≈ 6 bits, random hex ≈ 4 bits,
/// English text ≈ 1–2 bits. 3.5 catches most credential-like strings with low FP rate.
const MIN_ENTROPY: f64 = 3.5;
/// How far to look back (in bytes) for a context keyword.
const CONTEXT_WINDOW: usize = 100;

/// Context words that indicate a nearby high-entropy string is likely a credential.
static CONTEXT_WORDS: &[&str] = &[
    "token",
    "key",
    "secret",
    "password",
    "passwd",
    "credential",
    "auth",
    "bearer",
    "api_key",
    "access_key",
    "private",
    "signing",
    "hmac",
    "jwt",
    "session",
    "refresh",
    "client_secret",
];

/// Scan `text` for high-entropy strings near credential context words.
/// Returns hits sorted by start offset. Values are never stored — only positions and metadata.
pub fn entropy_scan(text: &str) -> Vec<EntropyHit> {
    let mut hits = Vec::new();
    let text_lower = text.to_lowercase();

    // Collect candidate tokens: runs of "credential-like" chars (alphanumeric + -_+/=.)
    // that are long enough to be suspicious.
    let mut token_start: Option<usize> = None;
    for (i, ch) in text.char_indices() {
        let is_cred_char = ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '+' | '/' | '=' | '.' | '~');
        if is_cred_char {
            if token_start.is_none() {
                token_start = Some(i);
            }
        } else if let Some(start) = token_start.take() {
            let end = i;
            let token = &text[start..end];
            if token.len() >= MIN_LEN {
                let h = shannon_entropy(token);
                if h >= MIN_ENTROPY {
                    if let Some(ctx) = context_word_before(&text_lower, start) {
                        hits.push(EntropyHit {
                            start,
                            end,
                            entropy: h,
                            context_word: ctx,
                        });
                    }
                }
            }
        }
    }
    // Handle token that runs to end of string
    if let Some(start) = token_start {
        let token = &text[start..];
        if token.len() >= MIN_LEN {
            let h = shannon_entropy(token);
            if h >= MIN_ENTROPY {
                if let Some(ctx) = context_word_before(&text_lower, start) {
                    hits.push(EntropyHit {
                        start,
                        end: text.len(),
                        entropy: h,
                        context_word: ctx,
                    });
                }
            }
        }
    }

    hits
}

/// Compute Shannon entropy of a string in bits/char.
/// H = -Σ p_i * log2(p_i) where p_i = count(c) / len
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let len = s.len() as f64;
    let mut freq = [0u32; 256];
    for &b in s.as_bytes() {
        freq[b as usize] += 1;
    }
    freq.iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Return the first matching context word found in the CONTEXT_WINDOW bytes before `pos`
/// in `text_lower` (already lowercased). Returns None if no context word is found.
fn context_word_before(text_lower: &str, pos: usize) -> Option<&'static str> {
    let window_start = pos.saturating_sub(CONTEXT_WINDOW);
    let window = &text_lower[window_start..pos];
    CONTEXT_WORDS
        .iter()
        .find(|&&kw| window.contains(kw))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_entropy_near_context_word_is_hit() {
        // A random-looking base64 string after "token:"
        let text = r#"token: aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe"#;
        let hits = entropy_scan(text);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].context_word, "token");
    }

    #[test]
    fn high_entropy_without_context_word_is_not_hit() {
        // Same string but no context word nearby
        let text = "value: aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe";
        let hits = entropy_scan(text);
        assert_eq!(hits.len(), 0, "no context word → should not be flagged");
    }

    #[test]
    fn low_entropy_near_context_word_is_not_hit() {
        // English text near "key" — not suspicious
        let text = "key: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let hits = entropy_scan(text);
        assert_eq!(
            hits.len(),
            0,
            "low entropy near context word must not trigger"
        );
    }

    #[test]
    fn short_token_is_not_hit() {
        let text = "token: abc123";
        let hits = entropy_scan(text);
        assert_eq!(hits.len(), 0, "too short to be suspicious");
    }

    #[test]
    fn shannon_entropy_random_base64() {
        // Random base64 has ~6 bits/char
        let s = "aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe09+/==";
        let h = shannon_entropy(s);
        assert!(h >= 4.0, "random base64 should have high entropy: {}", h);
    }

    #[test]
    fn shannon_entropy_all_same_char() {
        let s = "a".repeat(20);
        let h = shannon_entropy(&s);
        assert_eq!(h, 0.0, "all same char = 0 entropy");
    }

    #[test]
    fn shannon_entropy_two_chars() {
        // 50/50 split of two chars = 1.0 bits/char
        let s = "ababababababababababab"; // 20 chars
        let h = shannon_entropy(s);
        assert!((h - 1.0).abs() < 1e-9, "50/50 two chars = 1.0 bits: {}", h);
    }

    #[test]
    fn context_word_password_triggers() {
        let text = "password: xK9mN2pQrLsT4vWyAzBcDeFgHiJkOuVw";
        let hits = entropy_scan(text);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].context_word, "password");
    }

    #[test]
    fn context_word_bearer_triggers() {
        let text = "Authorization: Bearer eyAzB5xKpQrNmTy7RwSuVoLqHiGfEdC";
        let hits = entropy_scan(text);
        assert!(!hits.is_empty(), "bearer auth header should be flagged");
    }

    #[test]
    fn vault_token_format_not_flagged() {
        // Already-anonymized vault tokens should NOT trigger entropy scan
        // (they're deterministic short hex nonces, low entropy)
        let text = "email: [EMAIL:a1b2c3] and secret: [SECRET:d4e5f6]";
        let hits = entropy_scan(text);
        assert_eq!(hits.len(), 0, "vault tokens are not high-entropy credentials");
    }

    #[test]
    fn multiple_hits_returned() {
        let text = concat!(
            "api_key: aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe ",
            "secret: xK9mN2pQrLsT4vWyAzBcDeFgHiJkOuVwXy"
        );
        let hits = entropy_scan(text);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn hit_start_end_covers_token() {
        let token = "aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe";
        let text = format!("token: {}", token);
        let hits = entropy_scan(&text);
        assert_eq!(hits.len(), 1);
        assert_eq!(&text[hits[0].start..hits[0].end], token);
    }

    #[test]
    fn context_in_window_before_token() {
        // Context word 90 bytes before token — still within CONTEXT_WINDOW (100)
        let prefix = format!("token{}", " ".repeat(85));
        let token = "aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe";
        let text = format!("{}{}", prefix, token);
        let hits = entropy_scan(&text);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn context_outside_window_not_hit() {
        // Context word 110 bytes before token — outside CONTEXT_WINDOW
        let prefix = format!("token{}", " ".repeat(105));
        let token = "aB3xQzPkNmTy7RwSuVoLqHiGfEdCbAjWe";
        let text = format!("{}{}", prefix, token);
        let hits = entropy_scan(&text);
        assert_eq!(
            hits.len(),
            0,
            "context word too far away must not trigger"
        );
    }
}
