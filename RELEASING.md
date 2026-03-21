# Releasing Pagerunner

Steps to cut a new release, including the NER model and binary artifacts.

---

## 1. Update the NER model hash (required before any `--features ner` release)

The NER model file (`ner.onnx`) must be hosted at the GitHub release URL before the
hash can be computed. Do this once per model update:

```bash
# a. Download or build the model locally.
# b. Compute its SHA-256:
shasum -a 256 ~/.pagerunner/models/ner.onnx

# c. Update MODEL_SHA256 in src/anonymizer/ner.rs:
#    Replace the placeholder (64 zeros) with the real 64-char lowercase hex digest.
#    Edit line ~10 in src/anonymizer/ner.rs:
#      pub const MODEL_SHA256: &str = "<paste-real-hash-here>";

# d. Verify the constant is correct:
cargo test --features ner -- test_model_sha256_const_is_64_hex_chars

# e. Test that the model loads correctly end-to-end:
cargo build --release --features ner
pagerunner download-model   # re-downloads from the release URL
pagerunner open-session personal --anonymize   # verify NER works live
```

---

## 2. Host the model files at the GitHub release

Create a GitHub release tagged `ner-v1` (or a new tag for model updates) and attach:

- `ner.onnx` — the ONNX NER model
- `tokenizer.json` — the matching tokenizer

The URLs must match the constants in `src/anonymizer/ner.rs`:
```
MODEL_URL     = https://github.com/Enreign/pagerunner/releases/download/ner-v1/ner.onnx
TOKENIZER_URL = https://github.com/Enreign/pagerunner/releases/download/ner-v1/tokenizer.json
```

---

## 3. Run the full test suite

```bash
cargo test                            # all unit + CLI tests
cargo test --features ner             # NER-specific unit tests
cargo test --test cli_tools_integration -- --ignored   # Chrome integration tests
```

All tests must pass. Record results in `docs/test-runs/YYYY-MM-DD-run-N.md`.

---

## 4. Tag and create the GitHub release

```bash
git tag v0.x.0
git push origin v0.x.0
```

This triggers `release.yml` automatically:

1. `create-release` job runs first — creates the GitHub release with auto-generated notes
2. Three build jobs run in parallel (`macos-arm64`, `macos-x86_64`, `linux-x86_64`)
3. Each job builds the binary, strips it, generates a `.sha256` checksum, and uploads both to the release

Artifacts attached automatically:
- `pagerunner-macos-arm64` + `pagerunner-macos-arm64.sha256`
- `pagerunner-macos-x86_64` + `pagerunner-macos-x86_64.sha256`
- `pagerunner-linux-x86_64` + `pagerunner-linux-x86_64.sha256`

No manual binary attachment is needed.

---

## 5. Verify the release

Once CI completes (~10 minutes), verify at `https://github.com/Enreign/pagerunner/releases`:
- All 6 artifacts are attached (3 binaries + 3 checksums)
- Release notes are generated and include the intro paragraph
- Release is marked as the latest release

```bash
gh release view v0.x.0
```

---

## 6. Post-release

- Update the `version` field in `Cargo.toml`
- Run `cargo test` again to confirm no regressions from version bump
