# English Support Plan (v0.2.0)

## Objective

Make DOCX Break Cleaner find and repair hidden paragraph breaks in English
DOCX files, with no setting to pick a language, while the Japanese results stay
exactly as they are now (42 certain / 24 review on the reference book).

## Problems found in v0.1.1

1. **Merging glues words together.** The merge appends the second paragraph's
   runs directly after the first paragraph's runs, and the preview is built the
   same way. That is correct for Japanese, but in English `the end of` +
   `the line` becomes `the end ofthe line`.
2. **No English word rules.** The certain tier is Katakana splits plus a fixed
   list of 38 Japanese words from *The Great Rebellion*. English gets only
   detached punctuation and `100 | %`.
3. **The best English clue is unused.** It doesn't check whether the next
   paragraph starts with a lowercase letter.
4. **Interface text says "Japanese OCR text".**

## Joining rule (per boundary, decided by the characters on each side)

- Japanese/Chinese characters on both sides: join with nothing (current behaviour).
- Line ends with a hyphen and the joined word is in the English word list
  (`transfor-` + `mation`): remove the hyphen, join with nothing.
- Line ends with a hyphen and the hyphenated compound is the real form
  (`self-` + `realization`): keep the hyphen, join with nothing.
- Mid-word split with no hyphen (`transfor` + `mation`): join with nothing.
- Everything else in Latin script: join with one space, unless the first
  paragraph already ends with whitespace or the second already starts with it.

The merge writes that space, or removes that hyphen, inside the existing runs.
It keeps the formatting of the run it changes. The preview shows exactly the
text the saved file will contain.

## English rules

**Certain (merge preselected):**

- Detached punctuation (already exists).
- Number separated from `%` (already exists).
- Hyphenated word split where the joined form is in the word list, and neither
  half is a word on its own.
- Word split with no hyphen where neither half is a word and the joined form is.

**Review (preselected guess shown, user decides):**

- Next paragraph starts with a lowercase letter and the previous one has no
  closing punctuation. Preselected as merge.
- Hyphenated split where both the joined form and the hyphenated form are words.
  Preselected as keep-hyphen.
- The current formatting rule (no closing punctuation, same style and indent).
  It already works for any language.

**Never flagged:** headings, lists, tables, and the existing complex-structure
exclusions. Also lines that look like verse: several short lines in a row that
each start with a capital letter.

Word list: a bundled offline English list (SCOWL, permissive licence), a few MB.
It is used only for the split-word checks. Still no network access and no AI.

## Code changes

- `src-tauri/src/rules.rs`: split into `rules/japanese.rs`, `rules/english.rs`
  and `rules/common.rs`. Each boundary goes to a set of rules chosen by the
  characters on either side of the break. `RuleMatch` gains a `join` field
  (`Nothing`, `Space`, `RemoveHyphen`).
- `src-tauri/src/docx.rs`: `merge_document_xml` applies `join` at the boundary.
  The repair command gets the join for each selected candidate, not just the
  paragraph number. `joined_text` is built from the same join.
- `src/main.js`, `index.html`: language-neutral wording and English reason
  strings.
- README: describe English support and remove "Japan-team only".

## Verification

- Unit tests for every join case: space, existing whitespace, removed hyphen,
  kept hyphen, CJK.
- Round-trip test: the saved file's text equals the preview text for each
  merged pair.
- Japanese regression: the reference book still gives exactly 42 certain and
  24 review, and the repaired output is byte-identical to v0.1.1 output.
- English golden test on a real OCR'd English DOCX (needs a sample from Hans).
  First a hand audit of a sample of boundaries to count catches, false alarms
  and misses, then the counts are locked into a test.
- `npm test`, `cargo test`, clippy, production build, a manual run of the dev
  app, then tag `v0.2.0` so the Windows workflow builds the installer.

## Open questions for Hans

1. A sample English DOCX with the problem (ideally two from different sources).
2. Are these files usually line-per-paragraph (every printed line is its own
   Word paragraph)? If so, a book will produce thousands of candidates. That
   would bring back the question of bulk actions, which were removed in v0.1.
