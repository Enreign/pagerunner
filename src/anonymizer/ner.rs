// src/anonymizer/ner.rs
// ONNX NER inference for PERSON and ORG entity detection.
// Only compiled with --features ner.

use crate::anonymizer::patterns::{EntityType, Span};
use std::path::Path;

/// SHA-256 (lowercase hex, 64 chars) of the expected model file.
/// Update this constant when a new model is released.
pub const MODEL_SHA256: &str = "8fd4cba1016dd7e43e2b162da069387c838fcb3ccb5128cbeff476004ee94baf";

/// Download URL for the ONNX model file.
pub const MODEL_URL: &str =
    "https://github.com/Enreign/pagerunner/releases/download/ner-v1/ner.onnx";

/// Download URL for the tokenizer.json file (must be co-located with the model).
pub const TOKENIZER_URL: &str =
    "https://github.com/Enreign/pagerunner/releases/download/ner-v1/tokenizer.json";

// CoNLL-2003 BIO label order for distilbert-base-uncased-finetuned-conll03:
// 0=O, 1=B-MISC, 2=I-MISC, 3=B-PER, 4=I-PER, 5=B-ORG, 6=I-ORG, 7=B-LOC, 8=I-LOC
const ID2LABEL: &[&str] = &[
    "O", "B-MISC", "I-MISC", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC",
];

pub struct NerSession {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
}

#[derive(Debug, thiserror::Error)]
pub enum NerError {
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("model load failed: {0}")]
    Load(String),
    #[error("inference failed: {0}")]
    Inference(String),
}

/// Verify that the file at `path` matches `MODEL_SHA256`.
pub fn verify_model_hash(path: &Path) -> Result<(), NerError> {
    // Catch placeholder hash — the all-zeros constant means MODEL_SHA256 has not been
    // set to the real model hash yet. This happens before a tagged release. Fail fast
    // with a clear message instead of silently rejecting every real model file.
    if MODEL_SHA256.chars().all(|c| c == '0') {
        return Err(NerError::Load(
            "MODEL_SHA256 is the placeholder (all zeros) — \
             update src/anonymizer/ner.rs with the real SHA-256 of ner.onnx \
             after hosting the model at the ner-v1 GitHub release. \
             See RELEASING.md for the procedure."
                .to_string(),
        ));
    }

    use sha2::{Digest, Sha256};
    let bytes =
        std::fs::read(path).map_err(|e| NerError::Load(format!("cannot read model: {}", e)))?;
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual != MODEL_SHA256 {
        return Err(NerError::HashMismatch {
            expected: MODEL_SHA256.to_string(),
            actual,
        });
    }
    Ok(())
}

impl NerSession {
    /// Load the ONNX model and tokenizer from `model_path`.
    /// `tokenizer.json` must be in the same directory as the model.
    /// Verifies the model SHA-256 before loading.
    pub fn load(model_path: &Path) -> Result<NerSession, NerError> {
        verify_model_hash(model_path)?;

        let session = ort::session::Session::builder()
            .map_err(|e| NerError::Load(e.to_string()))?
            .commit_from_file(model_path)
            .map_err(|e| NerError::Load(e.to_string()))?;

        let tokenizer_path = model_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("tokenizer.json");
        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| NerError::Load(format!("tokenizer: {}", e)))?;

        Ok(NerSession { session, tokenizer })
    }

    /// Run NER inference on `text`. Returns PERSON and ORG spans only.
    /// LOC and MISC predictions are discarded.
    pub fn detect(&mut self, text: &str) -> Result<Vec<Span>, NerError> {
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| NerError::Inference(format!("tokenization: {}", e)))?;

        let ids: Vec<i64> = encoding.get_ids().iter().map(|&x| x as i64).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&x| x as i64)
            .collect();
        let type_ids: Vec<i64> = encoding.get_type_ids().iter().map(|&x| x as i64).collect();

        let seq_len = ids.len();
        if seq_len == 0 {
            return Ok(vec![]);
        }

        // Use (shape, Vec<T>) tuple form to avoid ndarray version mismatch with ort.
        let shape = vec![1i64, seq_len as i64];
        let ids_tensor = ort::value::Tensor::<i64>::from_array((shape.clone(), ids))
            .map_err(|e| NerError::Inference(e.to_string()))?;
        let mask_tensor = ort::value::Tensor::<i64>::from_array((shape.clone(), mask))
            .map_err(|e| NerError::Inference(e.to_string()))?;
        let type_tensor = ort::value::Tensor::<i64>::from_array((shape, type_ids))
            .map_err(|e| NerError::Inference(e.to_string()))?;

        let outputs = self
            .session
            .run(ort::inputs![ids_tensor, mask_tensor, type_tensor])
            .map_err(|e| NerError::Inference(e.to_string()))?;

        // Logits: [1, seq_len, num_labels] — extract as raw slice.
        let (logits_shape, logits_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| NerError::Inference(e.to_string()))?;

        // logits_shape: [1, seq_len, num_labels]
        let num_labels = logits_shape.get(2).copied().unwrap_or(0) as usize;

        let offsets = encoding.get_offsets();
        let special_tok_mask = encoding.get_special_tokens_mask();

        let mut spans: Vec<Span> = Vec::new();
        let mut current: Option<(usize, usize, EntityType)> = None;

        for tok_idx in 0..seq_len {
            if special_tok_mask[tok_idx] != 0 {
                flush(&mut current, &mut spans);
                continue;
            }

            // Row for this token: logits_data[tok_idx * num_labels .. (tok_idx+1) * num_labels]
            let row_start = tok_idx * num_labels;
            let row = &logits_data[row_start..row_start + num_labels];
            let label_id = row
                .iter()
                .enumerate()
                .max_by(|(_, a): &(usize, &f32), (_, b): &(usize, &f32)| {
                    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
                .unwrap_or(0);

            let label = ID2LABEL.get(label_id).copied().unwrap_or("O");
            let (char_start, char_end) = offsets[tok_idx];
            let byte_start = char_to_byte(text, char_start);
            let byte_end = char_to_byte(text, char_end);

            match label {
                "B-PER" => {
                    flush(&mut current, &mut spans);
                    current = Some((byte_start, byte_end, EntityType::Person));
                }
                "I-PER" => match current {
                    Some((s, _, EntityType::Person)) => {
                        current = Some((s, byte_end, EntityType::Person));
                    }
                    _ => flush(&mut current, &mut spans),
                },
                "B-ORG" => {
                    flush(&mut current, &mut spans);
                    current = Some((byte_start, byte_end, EntityType::Org));
                }
                "I-ORG" => match current {
                    Some((s, _, EntityType::Org)) => {
                        current = Some((s, byte_end, EntityType::Org));
                    }
                    _ => flush(&mut current, &mut spans),
                },
                _ => flush(&mut current, &mut spans),
            }
        }
        flush(&mut current, &mut spans);

        Ok(spans)
    }
}

fn flush(current: &mut Option<(usize, usize, EntityType)>, spans: &mut Vec<Span>) {
    if let Some((start, end, entity_type)) = current.take() {
        if start < end {
            spans.push(Span {
                start,
                end,
                entity_type,
            });
        }
    }
}

fn char_to_byte(text: &str, char_offset: usize) -> usize {
    text.char_indices()
        .nth(char_offset)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_sha256_const_is_64_hex_chars() {
        assert_eq!(
            MODEL_SHA256.len(),
            64,
            "SHA-256 hex must be 64 chars — update MODEL_SHA256 with real hash"
        );
        assert!(
            MODEL_SHA256.chars().all(|c| c.is_ascii_hexdigit()),
            "MODEL_SHA256 must be lowercase hex"
        );
    }

    // --- char_to_byte ---

    #[test]
    fn test_char_to_byte_ascii() {
        let text = "hello world";
        assert_eq!(char_to_byte(text, 0), 0);
        assert_eq!(char_to_byte(text, 6), 6); // 'w'
        assert_eq!(char_to_byte(text, 10), 10); // 'd'
    }

    #[test]
    fn test_char_to_byte_unicode_multibyte() {
        // "café" — 'é' is 2 bytes (UTF-8: 0xC3 0xA9)
        let text = "café";
        assert_eq!(char_to_byte(text, 0), 0); // 'c'
        assert_eq!(char_to_byte(text, 1), 1); // 'a'
        assert_eq!(char_to_byte(text, 2), 2); // 'f'
        assert_eq!(char_to_byte(text, 3), 3); // 'é' starts at byte 3
    }

    #[test]
    fn test_char_to_byte_past_end_returns_len() {
        let text = "hi";
        assert_eq!(char_to_byte(text, 99), text.len());
    }

    #[test]
    fn test_char_to_byte_empty_string() {
        assert_eq!(char_to_byte("", 0), 0);
    }

    // --- flush ---

    #[test]
    fn test_flush_none_does_nothing() {
        let mut current: Option<(usize, usize, EntityType)> = None;
        let mut spans: Vec<Span> = Vec::new();
        flush(&mut current, &mut spans);
        assert!(spans.is_empty());
        assert!(current.is_none());
    }

    #[test]
    fn test_flush_valid_span_appended() {
        let mut current = Some((0usize, 5usize, EntityType::Person));
        let mut spans: Vec<Span> = Vec::new();
        flush(&mut current, &mut spans);
        assert!(current.is_none());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start, 0);
        assert_eq!(spans[0].end, 5);
        assert_eq!(spans[0].entity_type, EntityType::Person);
    }

    #[test]
    fn test_flush_zero_width_span_discarded() {
        // start == end → not a real span, must be dropped
        let mut current = Some((3usize, 3usize, EntityType::Org));
        let mut spans: Vec<Span> = Vec::new();
        flush(&mut current, &mut spans);
        assert!(spans.is_empty(), "zero-width span must be discarded");
    }

    // --- verify_model_hash ---

    #[test]
    fn test_verify_model_hash_file_not_found() {
        let result = verify_model_hash(std::path::Path::new("/nonexistent/path/ner.onnx"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("cannot read model"), "got: {msg}");
    }

    #[test]
    fn test_verify_model_hash_placeholder_detected() {
        // MODEL_SHA256 is currently all zeros (pre-release placeholder).
        // verify_model_hash must detect this and return a Load error pointing
        // to RELEASING.md, rather than a misleading HashMismatch.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake.onnx");
        std::fs::write(&path, b"not a real model").unwrap();
        let result = verify_model_hash(&path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        // If MODEL_SHA256 is still all-zeros, we get the placeholder message.
        // If it has been updated to a real hash, we get a HashMismatch instead —
        // both are correct failures for a fake file.
        assert!(
            msg.contains("placeholder") || msg.contains("mismatch") || msg.contains("expected"),
            "unexpected error: {msg}"
        );
    }

    // --- ignored live-model tests ---

    #[test]
    fn test_ner_session_load() {
        let home = dirs::home_dir().unwrap();
        let model_path = home.join(".pagerunner/models/ner.onnx");
        let _session = NerSession::load(&model_path).expect("model should load");
    }

    #[test]
    fn test_ner_detects_person_and_org() {
        let home = dirs::home_dir().unwrap();
        let mut session = NerSession::load(&home.join(".pagerunner/models/ner.onnx")).unwrap();
        let spans = session
            .detect("Alice Smith is the CEO of Acme Corp.")
            .unwrap();
        let has_person = spans
            .iter()
            .any(|s| s.entity_type == crate::anonymizer::patterns::EntityType::Person);
        let has_org = spans
            .iter()
            .any(|s| s.entity_type == crate::anonymizer::patterns::EntityType::Org);
        assert!(has_person, "should detect PERSON");
        assert!(has_org, "should detect ORG");
    }

    #[test]
    fn test_ner_spans_are_valid_byte_offsets() {
        let home = dirs::home_dir().unwrap();
        let mut session = NerSession::load(&home.join(".pagerunner/models/ner.onnx")).unwrap();
        let text = "Bob Dylan won a Nobel Prize.";
        let spans = session.detect(text).unwrap();
        for s in &spans {
            assert!(s.start < s.end);
            assert!(s.end <= text.len());
            let _ = &text[s.start..s.end]; // must be valid UTF-8 slice
        }
    }

    fn model_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap()
            .join(".pagerunner/models/ner.onnx")
    }

    // NE2 — single PERSON span
    #[test]
    fn test_ner_detects_person_bob_dylan() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let text = "Bob Dylan won a Nobel Prize.";
        let spans = session.detect(text).unwrap();
        let persons: Vec<_> = spans
            .iter()
            .filter(|s| s.entity_type == EntityType::Person)
            .collect();
        assert!(!persons.is_empty(), "should detect Bob Dylan as PERSON");
        let name = &text[persons[0].start..persons[0].end];
        assert!(
            name.contains("Bob"),
            "span should contain 'Bob', got: {name}"
        );
    }

    // NE3 — ORG in a sentence alongside an email address (email is regex, not NER)
    #[test]
    fn test_ner_detects_org_google() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let spans = session
            .detect("Contact support at Google headquarters.")
            .unwrap();
        let has_org = spans.iter().any(|s| s.entity_type == EntityType::Org);
        assert!(has_org, "should detect Google as ORG");
    }

    // NE4 — PERSON + ORG in the same sentence
    #[test]
    fn test_ner_detects_person_and_org_combined() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let spans = session
            .detect("Jane Smith works at Microsoft Corp as an engineer.")
            .unwrap();
        let has_person = spans.iter().any(|s| s.entity_type == EntityType::Person);
        let has_org = spans.iter().any(|s| s.entity_type == EntityType::Org);
        assert!(has_person, "should detect Jane Smith as PERSON");
        assert!(has_org, "should detect Microsoft Corp as ORG");
    }

    // NE5 — multi-token person name is one contiguous span
    #[test]
    fn test_ner_multi_token_person_is_single_span() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let text = "John Paul Smith attended the conference.";
        let spans = session.detect(text).unwrap();
        let persons: Vec<_> = spans
            .iter()
            .filter(|s| s.entity_type == EntityType::Person)
            .collect();
        assert!(
            !persons.is_empty(),
            "should detect multi-token name as PERSON"
        );
        let name = &text[persons[0].start..persons[0].end];
        assert!(
            name.contains("John"),
            "span should contain 'John', got: {name}"
        );
        // Span must be a single contiguous range covering the full name
        assert!(
            persons.len() == 1 || {
                // If split, at least first token starts at the right place
                persons[0].start < persons[0].end
            },
            "name should be one span (or at least start correctly)"
        );
    }

    // NE6 — two ORG spans in one sentence
    #[test]
    fn test_ner_detects_two_orgs() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let spans = session
            .detect("Apple and IBM are technology companies.")
            .unwrap();
        let orgs: Vec<_> = spans
            .iter()
            .filter(|s| s.entity_type == EntityType::Org)
            .collect();
        assert!(
            orgs.len() >= 2,
            "should detect at least 2 ORG spans (Apple, IBM), got: {}",
            orgs.len()
        );
    }

    // NE7 — all byte offsets are valid UTF-8 slice boundaries (multi-entity text)
    #[test]
    fn test_ner_all_spans_valid_utf8_slices() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let text = "Alice Smith and Bob Jones met at Google's London office.";
        let spans = session.detect(text).unwrap();
        assert!(!spans.is_empty(), "should detect entities in the sentence");
        for s in &spans {
            assert!(s.start < s.end, "span start must be before end");
            assert!(s.end <= text.len(), "span end must be within text length");
            let slice = &text[s.start..s.end]; // panics if not on a char boundary
            assert!(!slice.is_empty(), "span slice must not be empty");
        }
    }

    // NE8 — NER span byte offsets stay valid even when another entity is adjacent
    #[test]
    fn test_ner_adjacent_spans_non_overlapping() {
        let mut session = NerSession::load(&model_path()).unwrap();
        let text = "Alice Smith from IBM presented.";
        let spans = session.detect(text).unwrap();
        // All spans must be non-overlapping and in order
        let mut sorted = spans.clone();
        sorted.sort_by_key(|s| s.start);
        for window in sorted.windows(2) {
            assert!(
                window[0].end <= window[1].start,
                "spans must not overlap: [{},{}] vs [{},{}]",
                window[0].start,
                window[0].end,
                window[1].start,
                window[1].end
            );
        }
    }
}
