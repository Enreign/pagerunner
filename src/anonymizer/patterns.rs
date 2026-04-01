// src/anonymizer/patterns.rs

use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EntityType {
    Email,
    Phone,
    CreditCard,
    Iban,
    Ssn,
    Ip,
    Person,         // detected by NER, not regex
    Org,            // detected by NER, not regex
    Secret,         // API keys, tokens, private keys — scrubbed before content reaches LLM
    Custom(String), // custom pattern name
}

#[derive(Debug, Clone)]
pub struct Span {
    pub start: usize, // byte offset
    pub end: usize,   // byte offset
    pub entity_type: EntityType,
}

// Regex patterns
static RE_EMAIL: OnceLock<Regex> = OnceLock::new();
static RE_PHONE: OnceLock<Regex> = OnceLock::new();
static RE_CREDIT_CARD: OnceLock<Regex> = OnceLock::new();
static RE_IBAN: OnceLock<Regex> = OnceLock::new();
static RE_SSN: OnceLock<Regex> = OnceLock::new();
static RE_IPV4: OnceLock<Regex> = OnceLock::new();

fn re_email() -> &'static Regex {
    RE_EMAIL
        .get_or_init(|| Regex::new(r"[a-zA-Z0-9._%+\-]+@[a-zA-Z0-9.\-]+\.[a-zA-Z]{2,}").unwrap())
}

fn re_phone() -> &'static Regex {
    RE_PHONE.get_or_init(|| {
        // US/international formats: +1-555-867-5309, 555-867-5309, (555) 867-5309, etc.
        Regex::new(r"(?:\+?1[-.\s]?)?\(?\d{3}\)?[-.\s]\d{3}[-.\s]\d{4}").unwrap()
    })
}

fn re_credit_card() -> &'static Regex {
    RE_CREDIT_CARD.get_or_init(|| {
        // 13-19 digit groups, possibly space/dash separated
        Regex::new(r"\b(?:\d{4}[-\s]?){3}\d{1,7}\b|\b\d{13,19}\b").unwrap()
    })
}

fn re_iban() -> &'static Regex {
    RE_IBAN.get_or_init(|| {
        // Country code (2 letters) + 2 check digits + up to 30 alphanumeric
        Regex::new(r"\b[A-Z]{2}\d{2}[A-Z0-9]{4,30}\b").unwrap()
    })
}

fn re_ssn() -> &'static Regex {
    RE_SSN.get_or_init(|| Regex::new(r"\b(\d{3})-(\d{2})-(\d{4})\b").unwrap())
}

fn re_ipv4() -> &'static Regex {
    RE_IPV4.get_or_init(|| Regex::new(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b").unwrap())
}

// ── Secret / credential patterns ──────────────────────────────────────────
// Tier 1: distinctive prefix + fixed length — very low false positive rate.
// Order matters for `entity_priority`; Secret wins over all PII types.
//
// Pattern design principles:
//   - Require word boundary or non-alphanumeric context to avoid partial matches
//   - Exact lengths enforced where known (reduces false positives dramatically)
//   - AWS secret access key excluded (40-char base64 is too ambiguous alone)

/// Combined secret pattern: one alternation covers all Tier 1 credential formats.
static RE_SECRET: OnceLock<Regex> = OnceLock::new();

fn re_secret() -> &'static Regex {
    RE_SECRET.get_or_init(|| {
        Regex::new(
            r"(?x)
            # npm tokens
            \bnpm_[A-Za-z0-9]{36}\b
            |
            # GitHub classic PATs (ghp_ / gho_ / ghu_ / ghs_ / ghr_)
            \bgh[pousr]_[A-Za-z0-9]{36}\b
            |
            # GitHub fine-grained PATs
            \bgithub_pat_[A-Za-z0-9]{22}_[A-Za-z0-9]{59}\b
            |
            # GitLab tokens (PAT glpat-, CI job glcbt-, deploy gldt-, feed glft-,
            #                runner auth glrt-, pipeline trigger glptt-, OAuth gloas-)
            \bgl(?:pat|cbt|dt|ft|rt|ptt|oas)-[A-Za-z0-9\-_]{20,}\b
            |
            # Stripe keys — sk_/pk_/rk_ with live/test/prod; length varies (10-99)
            \b(?:sk|pk|rk)_(?:live|test|prod)_[0-9A-Za-z]{10,99}\b
            |
            # Anthropic API keys (sk-ant-api03-* and sk-ant-admin01-* and future prefixes)
            \bsk-ant-[A-Za-z0-9\-_]{95}\b
            |
            # OpenAI classic keys — embed T3BlbkFJ anchor (base64 of OpenAI) to avoid
            # false positives on arbitrary 48-char alphanumeric strings
            \bsk-[A-Za-z0-9]{20}T3BlbkFJ[A-Za-z0-9]{20}\b
            |
            # OpenAI project / service-account / admin keys (longer structured format)
            \bsk-(?:proj|svcacct|admin)-[A-Za-z0-9_\-]{50,}\b
            |
            # Hugging Face user access tokens (hf_) and org API tokens (api_org_)
            \bhf_[A-Za-z0-9]{34,40}\b
            |
            \bapi_org_[A-Za-z0-9]{34}\b
            |
            # Google API keys
            \bAIza[0-9A-Za-z\-_]{35}\b
            |
            # AWS access key IDs: long-term (AKIA), STS (ASIA), assumed-role (ABIA),
            #                     cross-account (ACCA)
            \b(?:AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}\b
            |
            # Slack bot/user/workspace/legacy tokens
            \bxox[bpars]-[0-9A-Za-z\-]+\b
            |
            # Slack app-level tokens (xapp-1-AXXXXXX-timestamp-hex)
            \bxapp-\d-[A-Z0-9]+-\d+-[a-z0-9]+\b
            |
            # SendGrid API keys (SG. prefix, 66 more chars)
            \bSG\.[A-Za-z0-9\-_]{22}\.[A-Za-z0-9\-_]{43}\b
            |
            # Twilio API key SIDs
            \bSK[0-9a-fA-F]{32}\b
            |
            # Firebase legacy server keys
            \bAAAA[A-Za-z0-9_\-]{7}:[A-Za-z0-9_\-]{140}\b
            |
            # HashiCorp Vault service tokens (hvs.) and batch tokens (hvb.)
            \bhv[sb]\.[A-Za-z0-9_\-]{90,}\b
            |
            # Linear API keys
            \blin_api_[A-Za-z0-9]{40}\b
            |
            # PEM private key headers — any type prefix or bare PKCS8
            # Covers: RSA, EC, DSA, OPENSSH, ENCRYPTED, and bare PKCS8 (no type prefix)
            -----BEGIN\s(?:[A-Z0-9]+\s)*PRIVATE\sKEY(?:\sBLOCK)?-----
            |
            # JWT tokens (three base64url segments separated by dots)
            \beyJ[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_]+
        ",
        )
        .unwrap()
    })
}

/// Compiled custom pattern (regex or literal).
#[derive(Debug, Clone)]
pub struct CompiledCustomPattern {
    pub name: String,
    pub regex: Regex,
}

/// Detect PII spans in `text`. Only entity types in `entity_types` are scanned.
/// `custom_patterns` are applied after built-in types.
/// Returns deduplicated spans sorted by start offset.
pub fn detect_spans(
    text: &str,
    entity_types: &[EntityType],
    custom_patterns: &[CompiledCustomPattern],
) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();

    for entity_type in entity_types {
        let matches = match entity_type {
            EntityType::Email => collect_matches(text, re_email(), |_| true, EntityType::Email),
            EntityType::Phone => collect_matches(text, re_phone(), |_| true, EntityType::Phone),
            EntityType::CreditCard => collect_matches(
                text,
                re_credit_card(),
                |m| luhn_valid(&m.replace([' ', '-'], "")),
                EntityType::CreditCard,
            ),
            EntityType::Iban => collect_matches(
                text,
                re_iban(),
                |m| iban_valid(&m.replace(' ', "")),
                EntityType::Iban,
            ),
            EntityType::Ssn => collect_matches(text, re_ssn(), ssn_valid, EntityType::Ssn),
            EntityType::Ip => collect_matches(text, re_ipv4(), ipv4_valid, EntityType::Ip),
            EntityType::Secret => {
                collect_matches(text, re_secret(), |_| true, EntityType::Secret)
            }
            EntityType::Person | EntityType::Org => vec![], // NER detection happens in AnonEngine::process
            EntityType::Custom(_) => vec![],                // custom handled below
        };
        spans.extend(matches);
    }

    // Custom patterns
    for cp in custom_patterns {
        for m in cp.regex.find_iter(text) {
            spans.push(Span {
                start: m.start(),
                end: m.end(),
                entity_type: EntityType::Custom(cp.name.clone()),
            });
        }
    }

    deduplicate_spans(spans)
}

fn collect_matches<F>(text: &str, re: &Regex, validate: F, entity_type: EntityType) -> Vec<Span>
where
    F: Fn(&str) -> bool,
{
    re.find_iter(text)
        .filter(|m| validate(m.as_str()))
        .map(|m| Span {
            start: m.start(),
            end: m.end(),
            entity_type: entity_type.clone(),
        })
        .collect()
}

/// Deduplicate overlapping spans: longest wins. Ties: entity type priority order.
/// Priority: CreditCard > Iban > Email > Phone > Ssn > Ip > Person > Org > Custom
pub(crate) fn deduplicate_spans(mut spans: Vec<Span>) -> Vec<Span> {
    if spans.is_empty() {
        return spans;
    }

    // Sort by start, then by length (longest first), then by priority
    spans.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| (b.end - b.start).cmp(&(a.end - a.start)))
            .then_with(|| entity_priority(&a.entity_type).cmp(&entity_priority(&b.entity_type)))
    });

    let mut result: Vec<Span> = Vec::new();
    for span in spans {
        if let Some(last) = result.last() {
            if span.start < last.end {
                // Overlapping: skip (the already-added span won due to sort order)
                continue;
            }
        }
        result.push(span);
    }
    result
}

fn entity_priority(e: &EntityType) -> u8 {
    match e {
        // Secrets win over everything — a credential is never a false positive for an email
        EntityType::Secret => 0,
        EntityType::CreditCard => 1,
        EntityType::Iban => 2,
        EntityType::Email => 3,
        EntityType::Phone => 4,
        EntityType::Ssn => 5,
        EntityType::Ip => 6,
        EntityType::Person => 7,
        EntityType::Org => 8,
        EntityType::Custom(_) => 9,
    }
}

/// Luhn checksum validation (digits only, no spaces/dashes).
/// For credit card detection, the regex already enforces 13-19 digit length.
pub fn luhn_valid(digits: &str) -> bool {
    let digits: Vec<u8> = digits
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c as u8 - b'0')
        .collect();
    if digits.is_empty() {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let doubled = d * 2;
                if doubled > 9 {
                    (doubled - 9) as u32
                } else {
                    doubled as u32
                }
            } else {
                d as u32
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// IBAN mod-97 validation. Input should have spaces removed.
pub fn iban_valid(iban: &str) -> bool {
    if iban.len() < 4 {
        return false;
    }
    // Move first 4 chars to end
    let rearranged = format!("{}{}", &iban[4..], &iban[..4]);
    // Convert letters to digits (A=10, B=11, ..., Z=35)
    let numeric: String = rearranged
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                format!("{}", c.to_ascii_uppercase() as u32 - 'A' as u32 + 10)
            } else {
                c.to_string()
            }
        })
        .collect();
    // Compute mod 97 using chunked big-integer arithmetic
    let mut remainder: u64 = 0;
    for ch in numeric.chars() {
        let digit = ch.to_digit(10).unwrap() as u64;
        remainder = (remainder * 10 + digit) % 97;
    }
    remainder == 1
}

fn ssn_valid(s: &str) -> bool {
    // Must match \d{3}-\d{2}-\d{4} pattern (already enforced by regex)
    let area: u32 = s[..3].parse().unwrap_or(999);
    // Invalid prefixes: 000, 666, 900-999
    !(area == 0 || area == 666 || area >= 900)
}

fn ipv4_valid(s: &str) -> bool {
    s.split('.').filter_map(|o| o.parse::<u16>().ok()).count() == 4
        && s.split('.')
            .all(|o| o.parse::<u16>().map(|n| n <= 255).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_email() {
        let spans = detect_spans(
            "Contact user@example.com for help",
            &[EntityType::Email],
            &[],
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity_type, EntityType::Email);
        assert_eq!(
            &"Contact user@example.com for help"[spans[0].start..spans[0].end],
            "user@example.com"
        );
    }

    #[test]
    fn test_detect_phone_us() {
        let spans = detect_spans("Call 555-867-5309 now", &[EntityType::Phone], &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity_type, EntityType::Phone);
    }

    #[test]
    fn test_detect_credit_card_luhn() {
        // Valid Luhn: 4532015112830366
        let spans = detect_spans("Card: 4532015112830366", &[EntityType::CreditCard], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_credit_card_invalid_luhn() {
        // Invalid Luhn: 4532015112830367
        let spans = detect_spans("Card: 4532015112830367", &[EntityType::CreditCard], &[]);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_detect_iban_valid() {
        // GB29 NWBK 6016 1331 9268 19
        let spans = detect_spans("IBAN: GB29NWBK60161331926819", &[EntityType::Iban], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_iban_invalid_mod97() {
        let spans = detect_spans("IBAN: GB00NWBK60161331926819", &[EntityType::Iban], &[]);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_detect_ssn_valid() {
        let spans = detect_spans("SSN: 123-45-6789", &[EntityType::Ssn], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_ssn_invalid_000_prefix() {
        let spans = detect_spans("SSN: 000-45-6789", &[EntityType::Ssn], &[]);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_detect_ssn_invalid_666_prefix() {
        let spans = detect_spans("SSN: 666-45-6789", &[EntityType::Ssn], &[]);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_detect_ssn_invalid_900_plus_prefix() {
        let spans = detect_spans("SSN: 987-45-6789", &[EntityType::Ssn], &[]);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_detect_ipv4() {
        let spans = detect_spans("Server at 192.168.1.1", &[EntityType::Ip], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_ipv4_invalid_octet() {
        let spans = detect_spans("Not IP: 999.1.1.1", &[EntityType::Ip], &[]);
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_span_deduplication_longest_wins() {
        // A credit card number that also matches as a series of phone numbers
        // The longer CC match should win
        let text = "4532015112830366";
        let spans = detect_spans(text, &[EntityType::CreditCard, EntityType::Phone], &[]);
        // Only one span covering the full number, and it should be CreditCard
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity_type, EntityType::CreditCard);
    }

    #[test]
    fn test_only_requested_entity_types_scanned() {
        let text = "user@example.com and 4532015112830366";
        // Only scan EMAIL
        let spans = detect_spans(text, &[EntityType::Email], &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity_type, EntityType::Email);
    }

    #[test]
    fn test_luhn_checksum() {
        assert!(luhn_valid("4532015112830366"));
        assert!(!luhn_valid("4532015112830367"));
        assert!(luhn_valid("79927398713")); // classic test vector
    }

    #[test]
    fn test_mod97_iban() {
        assert!(iban_valid("GB29NWBK60161331926819"));
        assert!(!iban_valid("GB00NWBK60161331926819"));
    }

    #[test]
    fn test_detect_spans_person_org_produce_no_regex_spans() {
        // Person and Org have no regex detection — detect_spans must not panic
        // and must return zero spans for these types.
        let spans = detect_spans(
            "Alice works at Acme Corp, email alice@example.com",
            &[EntityType::Person, EntityType::Org],
            &[],
        );
        assert_eq!(spans.len(), 0, "Person/Org must not produce regex spans");
    }

    #[test]
    fn test_entity_priority_person_org_below_ip() {
        let ip_prio = entity_priority(&EntityType::Ip);
        let person_prio = entity_priority(&EntityType::Person);
        let org_prio = entity_priority(&EntityType::Org);
        assert!(
            person_prio > ip_prio,
            "Person must have lower priority than Ip"
        );
        assert!(
            org_prio > person_prio,
            "Org must have lower priority than Person"
        );
    }

    #[test]
    fn test_detect_multiple_emails() {
        let text = "Send to alice@example.com and bob@corp.io for review";
        let spans = detect_spans(text, &[EntityType::Email], &[]);
        assert_eq!(spans.len(), 2);
        let values: Vec<&str> = spans.iter().map(|s| &text[s.start..s.end]).collect();
        assert!(values.contains(&"alice@example.com"));
        assert!(values.contains(&"bob@corp.io"));
    }

    #[test]
    fn test_detect_multiple_entity_types_in_one_call() {
        let text = "Email user@example.com, call 555-867-5309";
        let spans = detect_spans(text, &[EntityType::Email, EntityType::Phone], &[]);
        assert_eq!(spans.len(), 2);
        let types: Vec<&EntityType> = spans.iter().map(|s| &s.entity_type).collect();
        assert!(types.contains(&&EntityType::Email));
        assert!(types.contains(&&EntityType::Phone));
    }

    #[test]
    fn test_detect_phone_international_format() {
        let spans = detect_spans("Call +1-555-867-5309 today", &[EntityType::Phone], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_phone_parenthesis_format() {
        let spans = detect_spans("Reach us at (555) 867-5309", &[EntityType::Phone], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_ipv4_broadcast() {
        let spans = detect_spans("Broadcast: 255.255.255.255", &[EntityType::Ip], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_ipv4_loopback() {
        let spans = detect_spans("Loopback: 127.0.0.1", &[EntityType::Ip], &[]);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_detect_custom_pattern() {
        let cp = CompiledCustomPattern {
            name: "TICKET".to_string(),
            regex: regex::Regex::new(r"PROJ-\d+").unwrap(),
        };
        let spans = detect_spans("See ticket PROJ-1234 for details", &[], &[cp]);
        assert_eq!(spans.len(), 1);
        assert_eq!(
            spans[0].entity_type,
            EntityType::Custom("TICKET".to_string())
        );
        assert_eq!(
            &"See ticket PROJ-1234 for details"[spans[0].start..spans[0].end],
            "PROJ-1234"
        );
    }

    #[test]
    fn test_detect_custom_pattern_multiple_matches() {
        let cp = CompiledCustomPattern {
            name: "TICKET".to_string(),
            regex: regex::Regex::new(r"PROJ-\d+").unwrap(),
        };
        let text = "PROJ-1 and PROJ-2 and PROJ-3";
        let spans = detect_spans(text, &[], &[cp]);
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn test_deduplicate_non_overlapping_both_kept() {
        let spans = vec![
            Span {
                start: 0,
                end: 5,
                entity_type: EntityType::Email,
            },
            Span {
                start: 10,
                end: 15,
                entity_type: EntityType::Phone,
            },
        ];
        let result = deduplicate_spans(spans);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_deduplicate_same_start_longer_wins() {
        // Two spans starting at same offset — longer one wins
        let spans = vec![
            Span {
                start: 0,
                end: 3,
                entity_type: EntityType::Phone,
            },
            Span {
                start: 0,
                end: 10,
                entity_type: EntityType::CreditCard,
            },
        ];
        let result = deduplicate_spans(spans);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].end, 10, "longer span should win");
    }

    #[test]
    fn test_deduplicate_same_start_same_length_higher_priority_wins() {
        // Same start, same length: CreditCard (priority 0) beats Email (priority 2)
        let spans = vec![
            Span {
                start: 0,
                end: 10,
                entity_type: EntityType::Email,
            },
            Span {
                start: 0,
                end: 10,
                entity_type: EntityType::CreditCard,
            },
        ];
        let result = deduplicate_spans(spans);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].entity_type, EntityType::CreditCard);
    }

    #[test]
    fn test_luhn_known_test_vectors() {
        assert!(luhn_valid("4111111111111111")); // Visa test
        assert!(luhn_valid("5500005555555559")); // Mastercard test
        assert!(!luhn_valid("1234567890123456")); // invalid
        assert!(!luhn_valid("")); // empty
    }

    #[test]
    fn test_iban_valid_de() {
        // DE89370400440532013000 — a well-known German IBAN test vector
        assert!(iban_valid("DE89370400440532013000"));
    }

    #[test]
    fn test_iban_too_short() {
        assert!(!iban_valid("GB2"));
    }

    // ── Secret / credential pattern tests ─────────────────────────────────

    fn detect_secret(text: &str) -> Vec<Span> {
        detect_spans(text, &[EntityType::Secret], &[])
    }

    #[test]
    fn test_secret_npm_token() {
        // 36 alphanumeric chars after "npm_"
        let token = "npm_".to_string() + &"A".repeat(36);
        let spans = detect_secret(&format!("Your token: {}", token));
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity_type, EntityType::Secret);
    }

    #[test]
    fn test_secret_github_pat_classic() {
        let token = "ghp_".to_string() + &"a1b2c3".repeat(6); // 36 chars
        let spans = detect_secret(&format!("token: {}", token));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_github_pat_other_prefixes() {
        for prefix in &["gho_", "ghu_", "ghs_", "ghr_"] {
            let token = format!("{}{}", prefix, "a".repeat(36));
            let spans = detect_secret(&token);
            assert_eq!(spans.len(), 1, "prefix {} should match", prefix);
        }
    }

    #[test]
    fn test_secret_github_fine_grained() {
        let token = format!("github_pat_{}_{}",  "a".repeat(22), "b".repeat(59));
        let spans = detect_secret(&token);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_stripe_key() {
        let key = format!("sk_live_{}", "a1".repeat(12)); // 24 chars
        let spans = detect_secret(&format!("Stripe key: {}", key));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_stripe_prod_key() {
        let key = format!("sk_prod_{}", "a1".repeat(12));
        let spans = detect_secret(&format!("Stripe prod key: {}", key));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_stripe_long_key() {
        // Stripe key lengths vary — should match lengths beyond 24
        let key = format!("sk_live_{}", "a".repeat(50));
        let spans = detect_secret(&key);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_openai_classic_key() {
        // Classic OpenAI key: sk- + 20 chars + T3BlbkFJ + 20 chars = 48 chars total
        let key = format!("sk-{}T3BlbkFJ{}", "a".repeat(20), "b".repeat(20));
        let spans = detect_secret(&format!("openai key: {}", key));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_openai_project_key() {
        let key = format!("sk-proj-{}", "a".repeat(60));
        let spans = detect_secret(&format!("openai key: {}", key));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_openai_no_false_positive() {
        // sk- with 48 random chars but NO T3BlbkFJ anchor — must NOT match
        let key = format!("sk-{}", "a".repeat(48));
        let spans = detect_secret(&key);
        assert_eq!(spans.len(), 0, "sk- without T3BlbkFJ anchor must not match");
    }

    #[test]
    fn test_secret_anthropic_key() {
        // sk-ant- prefix + exactly 95 chars
        let key = format!("sk-ant-{}", "a1b2".repeat(23) + "abc"); // 92 + 3 = 95 chars
        let spans = detect_secret(&key);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_google_api_key() {
        // AIza prefix + exactly 35 chars (alphanumeric + - + _)
        let key = format!("AIza{}", "a1B2c".repeat(7)); // 35 chars
        let spans = detect_secret(&key);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_aws_iam_key() {
        let key = "AKIA1234567890ABCDEF"; // AKIA + 16 uppercase alphanumeric
        let spans = detect_secret(&format!("aws key id: {}", key));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_aws_sts_key() {
        let key = "ASIA1234567890ABCDEF";
        let spans = detect_secret(key);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_slack_bot_token() {
        let token = "xoxb-12345-67890-abcdefghijklmnop";
        let spans = detect_secret(&format!("slack: {}", token));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_slack_user_token() {
        let token = "xoxp-12345-67890-abcdefghijklmnop";
        let spans = detect_secret(token);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_slack_app_token() {
        let token = "xapp-1-A012BC3DE4F-1234567890123-abcdef1234567890abcdef1234567890abcdef";
        let spans = detect_secret(token);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_jwt_token() {
        // Realistic JWT structure: header.payload.signature
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
        let spans = detect_secret(&format!("Authorization: Bearer {}", token));
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_pem_private_key_header() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA...\n-----END RSA PRIVATE KEY-----";
        let spans = detect_secret(text);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_openssh_key_header() {
        let text = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAA...";
        let spans = detect_secret(text);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_pkcs8_bare_private_key() {
        // PKCS8 unencrypted: "BEGIN PRIVATE KEY" with no type prefix
        let text = "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC...";
        let spans = detect_secret(text);
        assert_eq!(spans.len(), 1, "bare PKCS8 private key must match");
    }

    #[test]
    fn test_secret_dsa_private_key() {
        let text = "-----BEGIN DSA PRIVATE KEY-----\nMIIBuwIBAAKBgQC...";
        let spans = detect_secret(text);
        assert_eq!(spans.len(), 1, "DSA private key must match");
    }

    #[test]
    fn test_secret_gitlab_additional_types() {
        for prefix in &["glcbt", "gldt", "glft", "glrt", "glptt", "gloas"] {
            let token = format!("{}-{}", prefix, "a1b2c3d4".repeat(3));
            let spans = detect_secret(&token);
            assert_eq!(spans.len(), 1, "GitLab {} token should match", prefix);
        }
    }

    #[test]
    fn test_secret_aws_additional_prefixes() {
        for prefix in &["ABIA", "ACCA"] {
            let key = format!("{}1234567890ABCDEF", prefix);
            let spans = detect_secret(&key);
            assert_eq!(spans.len(), 1, "AWS prefix {} should match", prefix);
        }
    }

    #[test]
    fn test_secret_hashicorp_vault_service_token() {
        let token = format!("hvs.{}", "A".repeat(90));
        let spans = detect_secret(&token);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_hashicorp_vault_batch_token() {
        let token = format!("hvb.{}", "A".repeat(95));
        let spans = detect_secret(&token);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_linear_api_key() {
        let key = format!("lin_api_{}", "a1b2c3d4".repeat(5)); // 40 chars
        let spans = detect_secret(&key);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_huggingface_org_token() {
        let key = format!("api_org_{}", "a".repeat(34));
        let spans = detect_secret(&key);
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_secret_no_false_positive_short_string() {
        // Short random string — should NOT match
        let spans = detect_secret("abc123");
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_secret_no_false_positive_email() {
        // Email should not match as Secret
        let spans = detect_secret("user@example.com");
        assert_eq!(spans.len(), 0);
    }

    #[test]
    fn test_secret_priority_beats_other_entities() {
        // AWS key starts with "AKIA" — should not be mistaken for another entity
        let key = "AKIA1234567890ABCDEF";
        let spans = detect_spans(key, &[EntityType::Secret, EntityType::Ip], &[]);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].entity_type, EntityType::Secret);
    }

    #[test]
    fn test_secret_entity_priority_is_highest() {
        use super::entity_priority;
        let secret_prio = entity_priority(&EntityType::Secret);
        for other in &[
            EntityType::CreditCard,
            EntityType::Iban,
            EntityType::Email,
            EntityType::Phone,
            EntityType::Ssn,
            EntityType::Ip,
            EntityType::Person,
            EntityType::Org,
        ] {
            assert!(
                secret_prio < entity_priority(other),
                "Secret priority ({}) must be lower number (higher priority) than {:?} ({})",
                secret_prio,
                other,
                entity_priority(other)
            );
        }
    }
}
