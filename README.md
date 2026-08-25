# DOCX Break Cleaner

An offline desktop utility for reviewing and repairing hidden paragraph breaks
introduced by OCR and PDF-to-DOCX conversion. This is a standalone Japan-team
tool and is not part of Gnosis TMS.

## Operator workflow

1. Drop a `.docx` file onto the app or choose it from disk.
2. Review the explicit `¶` markers showing each hidden Word paragraph boundary.
3. Choose **Merge** or **Don’t merge** for each finding. **Certain** findings
   default to Merge. **Review** findings start with a conservative mechanical
   best guess that the operator can override.
4. The preview switches immediately between joined text and the original
   two-paragraph layout.
5. Save a separate `.cleaned.docx` copy.

## Downloads

Installers for Windows x64 are published on the
[GitHub Releases page](https://github.com/gnosistms/docx-break-cleaner/releases).

The initial Windows installer is unsigned, so Microsoft Defender SmartScreen
may show a one-time warning when it is first opened.

## Safety model

- The original DOCX is never overwritten.
- Scanning is read-only.
- Only user-selected, structurally safe paragraph pairs are merged.
- Complex boundaries involving tables, tracked changes, comments, bookmarks,
  content controls, drawings, or section properties are excluded.
- A new `.cleaned.docx` file is written and structurally validated.

## Development

```bash
npm install
npm test
npm run tauri:dev
```

Rust tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

Run the local golden test against the audited reference document:

```bash
DOCX_CLEANER_REFERENCE="/path/to/The Great Rebellion 偉大なる反乱.docx" \
  cargo test --manifest-path src-tauri/Cargo.toml reference_document_counts -- --nocapture
```

## Distribution

```bash
npm run tauri:build
```

The included GitHub Actions workflow builds Windows x64 installers and attaches
them to tagged GitHub releases. macOS builds are currently for local development
and testing only.

Copyright © 2026 Gnosis TMS. All rights reserved. No open-source license is
granted by publication of this source code.
