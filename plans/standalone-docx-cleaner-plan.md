# Standalone DOCX Break Cleaner Plan

## Objective

Build a small offline desktop application for the Japan team that detects and
repairs OCR/PDF-conversion paragraph boundaries in DOCX files without adding
language-specific behavior to Gnosis TMS.

## Scope

1. Parse `word/document.xml` from a DOCX package without altering the source.
2. Detect two confidence tiers:
   - **Certain**: verified word/morpheme, unit, or detached-punctuation splits.
   - **Review**: formatting and paragraph-structure evidence suggests a continuation.
3. Present every candidate with an explicit visible paragraph marker and joined preview.
4. Let the user select repairs and write a new `.cleaned.docx` file.
5. Preserve unrelated package entries and refuse unsafe/complex boundaries.
6. Validate the output archive and XML before reporting success.
7. Package from one Tauri 2 codebase for Windows and macOS.

## Safety invariants

- Never overwrite the input file.
- Never mutate a document during scanning.
- Never merge across tables, sections, lists, headings, content controls, tracked
  changes, comments, bookmarks, drawings, or other unsupported structures.
- Keep the first paragraph's formatting and append the second paragraph's content.
- Preserve every unrelated ZIP entry.
- Write through a temporary file and rename only after validation.

## Verification

- Unit tests for scanning, exclusions, merging, and archive validation.
- Local golden test against `The Great Rebellion 偉大なる反乱.docx` using an
  environment-provided path; the document is not checked in.
- Expected golden result: 42 certain candidates and 24 review candidates.
- `cargo test`, frontend build, Tauri build, and manual smoke test.

