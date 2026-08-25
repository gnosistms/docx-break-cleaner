import assert from "node:assert/strict";
import test from "node:test";
import {
  decisionPreview,
  formatBytes,
  selectedCandidateIds,
  suggestedCandidateIds,
} from "../src/presentation.js";

test("selectedCandidateIds drops stale selections", () => {
  assert.deepEqual(
    selectedCandidateIds([{ id: "p1-p2" }], new Set(["p1-p2", "p9-p10"])),
    ["p1-p2"],
  );
});

test("suggestedCandidateIds applies best guesses without changing confidence", () => {
  assert.deepEqual(
    suggestedCandidateIds([
      { id: "certain", confidence: "certain", suggestedMerge: true },
      { id: "review-merge", confidence: "review", suggestedMerge: true },
      { id: "review-keep", confidence: "review", suggestedMerge: false },
    ]),
    ["certain", "review-merge"],
  );
});

test("decisionPreview shows merged text when merge is selected", () => {
  assert.equal(
    decisionPreview(
      { beforeText: "プログラ", afterText: "ムされた", joinedText: "プログラムされた" },
      true,
    ),
    "プログラムされた",
  );
});

test("decisionPreview shows a real line break when don't merge is selected", () => {
  assert.equal(
    decisionPreview(
      { beforeText: "何も知らない", afterText: "というのは", joinedText: "何も知らないというのは" },
      false,
    ),
    "何も知らない\nというのは",
  );
});

test("formatBytes uses readable units", () => {
  assert.equal(formatBytes(2048), "2.0 KB");
});
