# DOCX Break Cleaner

An offline desktop utility for reviewing and repairing hidden paragraph breaks
introduced by OCR and PDF-to-DOCX conversion. It is a standalone tool and is not
part of Gnosis TMS.

## What it detects

The rules are picked per boundary from the characters on each side; there is no
language setting.

- **Japanese and Chinese text:** words and inflections split across paragraphs,
  Katakana splits, detached punctuation, numbers split from units. Merged text is
  joined with nothing in between.
- **Space-separated scripts (English, Spanish, French…):** a line that stops
  without punctuation and continues in lowercase; a line that runs to the right
  margin without punctuation (in documents that store one printed line per
  paragraph); line-end hyphens. Merged text is joined with one space. A line-end
  hyphen is removed when the document spells the word elsewhere without it, and
  kept when the document spells the compound with it; otherwise it is left for
  review.
- **Page numbers inside a sentence:** a paragraph holding only a page number
  between two halves of a sentence is removed by that merge.

Breaks between complete sentences (a line ending in a full stop followed by a
capital letter) are never flagged.

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

Use the [download page](https://gnosistms.github.io/docx-break-cleaner/) for the
easiest platform-specific download, or browse the
[GitHub Releases page](https://github.com/gnosistms/docx-break-cleaner/releases).

The installers are not code-signed, so Microsoft Defender SmartScreen (Windows)
or Gatekeeper (macOS) may warn the first time the app is opened. After v0.2.0 is
installed, later versions arrive through the app's own update prompt.

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

Run the local English audit (prints counts by rule; optional JSON dump and
repaired copy):

```bash
DOCX_CLEANER_ENGLISH="/path/to/Fundamental Education SAW.docx" \
  DOCX_CLEANER_DUMP=/tmp/candidates.json DOCX_CLEANER_TEST_OUTPUT=/tmp/cleaned.docx \
  cargo test --manifest-path src-tauri/Cargo.toml english_reference_audit -- --nocapture
```

Run the local golden test against the audited Japanese reference document:

```bash
DOCX_CLEANER_REFERENCE="/path/to/The Great Rebellion 偉大なる反乱.docx" \
  cargo test --manifest-path src-tauri/Cargo.toml reference_document_counts -- --nocapture
```

## Distribution

The download site lives in `docs/` and is deployed to GitHub Pages by
`.github/workflows/pages.yml`. It detects the repository from the Pages URL and
links each platform button to the matching asset from the latest GitHub Release.

Pushing a `v*` tag runs `.github/workflows/build.yml`, which builds the Windows
x64 installers and the Apple silicon macOS `.dmg` and attaches them, with their
updater signatures and `latest.json`, to one GitHub Release. Neither platform is
code-signed by Microsoft or Apple, so SmartScreen and Gatekeeper warn on first
install.

## Updates

From v0.2.0 the app checks
`releases/latest/download/latest.json` once at startup and offers to install a
newer version. Only the version check goes over the network; documents never
leave the computer, and the check fails silently when offline. Updates must be
signed with the updater key:

- private key: `~/.tauri/docx-break-cleaner.key` (password beside it in
  `docx-break-cleaner.key.password`) on the maintainer's Mac, and in the
  repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`;
- public key: `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`.

If the private key is lost, installed copies can no longer be updated
automatically; users would have to download a new installer by hand.

Copyright © 2026 Gnosis TMS. All rights reserved. No open-source license is
granted by publication of this source code.
