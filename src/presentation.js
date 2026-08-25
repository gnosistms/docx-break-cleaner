export function decisionPreview(candidate, shouldMerge) {
  return shouldMerge
    ? candidate.joinedText
    : `${candidate.beforeText}\n${candidate.afterText}`;
}

export function formatBytes(value) {
  const bytes = Number(value) || 0;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function selectedCandidateIds(candidates, selected) {
  const validIds = new Set(candidates.map((candidate) => candidate.id));
  return [...selected].filter((id) => validIds.has(id));
}

export function suggestedCandidateIds(candidates) {
  return candidates
    .filter(
      (candidate) =>
        candidate.suggestedMerge ?? candidate.confidence === "certain",
    )
    .map((candidate) => candidate.id);
}
