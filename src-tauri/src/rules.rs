use crate::docx::ParagraphRecord;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Confidence {
    Certain,
    Review,
}

/// What goes between the two paragraphs' text when they are merged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Join {
    /// Append directly (CJK text, detached punctuation, kept compound hyphen).
    Nothing,
    /// Insert one space (wrapped line in a space-separated script).
    Space,
    /// Remove the line-end hyphen, then append directly (`transfor-` + `mation`).
    RemoveHyphen,
}

pub(crate) struct RuleMatch {
    pub(crate) confidence: Confidence,
    pub(crate) suggested_merge: bool,
    pub(crate) join: Join,
    pub(crate) code: &'static str,
    pub(crate) reason: &'static str,
}

/// Facts about the whole document that individual boundary rules need.
#[derive(Debug, Default)]
pub(crate) struct DocumentContext {
    /// Typical character count of a full printed line, when the document stores
    /// one printed line per Word paragraph. `None` for ordinary documents.
    pub(crate) line_width: Option<usize>,
    /// Lowercased words written without a hyphen anywhere in the document.
    pub(crate) words: HashSet<String>,
    /// Lowercased hyphenated compounds written within one paragraph.
    pub(crate) hyphenated: HashSet<String>,
}

impl DocumentContext {
    pub(crate) fn from_paragraphs(paragraphs: &[ParagraphRecord]) -> Self {
        let mut words = HashSet::new();
        let mut hyphenated = HashSet::new();
        let mut lengths = Vec::new();
        for paragraph in paragraphs {
            let text = paragraph.text.trim();
            if text.is_empty() || paragraph.is_page_number {
                continue;
            }
            if !text.contains('\n') {
                lengths.push(text.chars().count());
            }
            for token in text.split(|value: char| !(value.is_alphabetic() || value == '-')) {
                let token = token.trim_matches('-');
                if token.is_empty() {
                    continue;
                }
                let lower = token.to_lowercase();
                if lower.contains('-') {
                    hyphenated.insert(lower);
                } else {
                    words.insert(lower);
                }
            }
        }
        Self {
            line_width: line_width(&mut lengths),
            words,
            hyphenated,
        }
    }
}

/// A document stores one printed line per paragraph when most paragraphs are
/// about one line long. The 90th percentile is then the full-line width.
fn line_width(lengths: &mut [usize]) -> Option<usize> {
    if lengths.len() < 40 {
        return None;
    }
    lengths.sort_unstable();
    let median = lengths[lengths.len() / 2];
    let p90 = lengths[lengths.len() * 9 / 10];
    ((20..=120).contains(&median) && p90 <= 140).then_some(p90)
}

const VERIFIED_SPLIT_TOKENS: &[&str] = &[
    "流れ込んで",
    "流れ出る",
    "大惨事",
    "だろう",
    "発明された",
    "ため",
    "しかし",
    "掘っ立て小屋",
    "問題",
    "退化する",
    "対立する",
    "背中",
    "かかわらず",
    "立ち返り",
    "であり",
    "プログラム",
    "野性的",
    "訪れる",
    "コントロールされて",
    "憎むべき",
    "非常に",
    "起こる",
    "排除した",
    "操り人形",
    "クンダリーニ",
    "不活性",
    "不条理",
    "自己",
    "アブラクサス",
    "怒りっぽい",
    "なければ",
    "生きている",
    "淫乱者",
    "ルニック",
    "マジック",
    "いる",
    "秘教的",
    "実践的",
];

pub(crate) fn classify_boundary(
    previous: &ParagraphRecord,
    following: &ParagraphRecord,
    context: &DocumentContext,
) -> Option<RuleMatch> {
    let a = previous.text.trim();
    let b = following.text.trim();
    if a.is_empty() || b.is_empty() || previous.unsafe_content || following.unsafe_content {
        return None;
    }
    let cjk_boundary = a.chars().last().is_some_and(is_cjk) || b.chars().next().is_some_and(is_cjk);
    if cjk_boundary {
        classify_cjk_boundary(previous, following)
    } else {
        classify_spaced_boundary(previous, following, context)
    }
}

fn classify_cjk_boundary(
    previous: &ParagraphRecord,
    following: &ParagraphRecord,
) -> Option<RuleMatch> {
    let a = previous.text.trim();
    let b = following.text.trim();
    if a.is_empty() || b.is_empty() || previous.unsafe_content || following.unsafe_content {
        return None;
    }

    if is_detached_punctuation(b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            join: Join::Nothing,
            code: "detached_punctuation",
            reason:
                "The next paragraph contains only punctuation detached from the preceding text.",
        });
    }
    if is_number_unit_split(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            join: Join::Nothing,
            code: "number_unit_split",
            reason: "A number has been separated from its unit.",
        });
    }
    if let Some(token) = verified_token_crossing_boundary(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            join: Join::Nothing,
            code: "verified_token_split",
            reason: token_reason(token),
        });
    }
    if katakana_crosses_boundary(a, b) && continuation_format(previous, following) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            join: Join::Nothing,
            code: "katakana_token_split",
            reason: "A continuous Katakana token is split across two Word paragraphs.",
        });
    }

    if bibliography_continuation(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: true,
            join: Join::Nothing,
            code: "bibliography_continuation",
            reason: "The next paragraph appears to continue the same starred title.",
        });
    }

    if quoted_clause_continuation(previous, following) {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: false,
            join: Join::Nothing,
            code: "quoted_clause_continuation",
            reason:
                "A clause without terminal punctuation appears to continue into a quoted phrase.",
        });
    }

    if likely_visual_continuation(previous, following) {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: true,
            join: Join::Nothing,
            code: "visual_continuation",
            reason: "Paragraph formatting and incomplete text make this look like a hidden continuation.",
        });
    }
    None
}

/// Rules for scripts that put spaces between words (English, Spanish, French…).
/// They read only letter case, punctuation, hyphens and line length, never a
/// language's vocabulary.
fn classify_spaced_boundary(
    previous: &ParagraphRecord,
    following: &ParagraphRecord,
    context: &DocumentContext,
) -> Option<RuleMatch> {
    let a = previous.text.trim();
    let b = following.text.trim();
    let space = if previous.text.ends_with(char::is_whitespace)
        || following.text.starts_with(char::is_whitespace)
    {
        Join::Nothing
    } else {
        Join::Space
    };

    if is_detached_punctuation(b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            join: Join::Nothing,
            code: "detached_punctuation",
            reason:
                "The next paragraph contains only punctuation detached from the preceding text.",
        });
    }
    if is_number_unit_split(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            join: Join::Nothing,
            code: "number_unit_split",
            reason: "A number has been separated from its unit.",
        });
    }
    if previous.format.is_list
        || following.format.is_list
        || previous.format.is_heading
        || following.format.is_heading
        || previous.format.style != following.format.style
    {
        return None;
    }
    let next_starts_lowercase = first_letter(b).is_some_and(char::is_lowercase);
    let same_look = previous.format.last_run == following.format.first_run;

    if let Some(left) = line_end_hyphen_word(a) {
        if next_starts_lowercase {
            let right = leading_word(b);
            let joined = format!("{left}{right}").to_lowercase();
            let compound = format!("{left}-{right}").to_lowercase();
            let joined_seen = context.words.contains(&joined);
            let compound_seen = context.hyphenated.contains(&compound);
            if joined_seen && !compound_seen && previous.hyphen_removable {
                return Some(RuleMatch {
                    confidence: Confidence::Certain,
                    suggested_merge: true,
                    join: Join::RemoveHyphen,
                    code: "hyphenated_word_split",
                    reason: "A word is hyphenated at the line end; the document spells it elsewhere without the hyphen.",
                });
            }
            if compound_seen && !joined_seen {
                return Some(RuleMatch {
                    confidence: Confidence::Certain,
                    suggested_merge: true,
                    join: Join::Nothing,
                    code: "hyphenated_compound_split",
                    reason: "A hyphenated compound is split at its hyphen; the document spells it elsewhere with the hyphen.",
                });
            }
            return Some(RuleMatch {
                confidence: Confidence::Review,
                suggested_merge: true,
                join: if previous.hyphen_removable {
                    Join::RemoveHyphen
                } else {
                    Join::Nothing
                },
                code: "line_end_hyphen",
                reason: "A word is hyphenated at the line end. Check whether the hyphen belongs in the word.",
            });
        }
        if same_look {
            return Some(RuleMatch {
                confidence: Confidence::Certain,
                suggested_merge: true,
                join: Join::Nothing,
                code: "hyphenated_compound_split",
                reason: "A hyphenated compound is split at its hyphen.",
            });
        }
    }

    let line_ratio = context
        .line_width
        .map(|width| last_line(a).chars().count() * 100 / width.max(1));

    if !ends_with_sentence_punctuation(a) {
        if next_starts_lowercase {
            return Some(RuleMatch {
                confidence: Confidence::Certain,
                suggested_merge: true,
                join: space,
                code: "lowercase_continuation",
                reason:
                    "The line stops without punctuation and the next paragraph starts in lowercase.",
            });
        }
        return match line_ratio {
            Some(ratio) if ratio >= 85 && same_look => Some(RuleMatch {
                confidence: Confidence::Certain,
                suggested_merge: true,
                join: space,
                code: "full_line_continuation",
                reason: "The line runs to the right margin and stops without punctuation.",
            }),
            Some(ratio) if ratio >= 50 && same_look => Some(RuleMatch {
                confidence: Confidence::Review,
                suggested_merge: true,
                join: space,
                code: "partial_line_continuation",
                reason: "The line stops without punctuation, but it does not reach the right margin.",
            }),
            Some(_) => None,
            None => likely_visual_continuation(previous, following).then_some(RuleMatch {
                confidence: Confidence::Review,
                suggested_merge: true,
                join: space,
                code: "visual_continuation",
                reason: "Paragraph formatting and incomplete text make this look like a hidden continuation.",
            }),
        };
    }

    if next_starts_lowercase {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: true,
            join: space,
            code: "lowercase_after_punctuation",
            reason: "The next paragraph starts in lowercase after punctuation.",
        });
    }
    None
}

/// The word before a line-end hyphen, when the paragraph ends `letters-`.
fn line_end_hyphen_word(value: &str) -> Option<&str> {
    let stem = value.strip_suffix('-')?;
    if stem.ends_with('-') {
        return None;
    }
    let start = stem
        .char_indices()
        .rev()
        .find(|(_, value)| !value.is_alphabetic())
        .map(|(index, value)| index + value.len_utf8())
        .unwrap_or(0);
    let word = &stem[start..];
    (!word.is_empty()).then_some(word)
}

fn leading_word(value: &str) -> &str {
    let end = value
        .char_indices()
        .find(|(_, value)| !value.is_alphabetic())
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    &value[..end]
}

fn first_letter(value: &str) -> Option<char> {
    value
        .chars()
        .find(|value| !"\"“‘'(«[¿¡".contains(*value))
        .filter(|value| value.is_alphabetic())
}

fn last_line(value: &str) -> &str {
    value.lines().last().unwrap_or(value).trim()
}

fn ends_with_sentence_punctuation(value: &str) -> bool {
    value
        .trim_end_matches(|value: char| "\"”’'»)]".contains(value))
        .chars()
        .last()
        .is_some_and(|last| ".!?:;…".contains(last))
}

fn is_cjk(value: char) -> bool {
    matches!(value,
        '\u{3000}'..='\u{30FF}'
        | '\u{31F0}'..='\u{31FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FF00}'..='\u{FFEF}'
        | '\u{20000}'..='\u{3FFFF}')
}

fn quoted_clause_continuation(a: &ParagraphRecord, b: &ParagraphRecord) -> bool {
    let text_a = a.text.trim();
    let text_b = b.text.trim();
    text_a.chars().count() >= 10
        && !ends_with_terminal(text_a)
        && matches!(text_b.chars().next(), Some('「' | '『' | '“' | '‘'))
        && !a.format.is_list
        && !b.format.is_list
        && !a.format.is_heading
        && !b.format.is_heading
        && a.format.style == b.format.style
        && match (a.format.left_indent, b.format.left_indent) {
            (Some(left), Some(right)) => (left - right).abs() <= 8.0,
            _ => true,
        }
}

fn verified_token_crossing_boundary(a: &str, b: &str) -> Option<&'static str> {
    for token in VERIFIED_SPLIT_TOKENS {
        for (byte_index, _) in token.char_indices().skip(1) {
            let (left, right) = token.split_at(byte_index);
            if a.ends_with(left) && b.starts_with(right) {
                return Some(token);
            }
        }
    }
    None
}

fn token_reason(token: &str) -> &'static str {
    if token.chars().all(is_katakana) {
        "A verified Katakana word is split across two Word paragraphs."
    } else {
        "A verified Japanese word or inflected form is split across two Word paragraphs."
    }
}

fn is_detached_punctuation(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    !chars.is_empty()
        && chars.len() <= 3
        && chars
            .iter()
            .all(|value| "。！？.!?」』）)]】〉》、，,;:；：".contains(*value))
}

fn is_number_unit_split(a: &str, b: &str) -> bool {
    a.chars().last().is_some_and(|value| value.is_ascii_digit())
        && ["%", "％", "パーセント"]
            .iter()
            .any(|unit| b.starts_with(unit))
}

fn katakana_crosses_boundary(a: &str, b: &str) -> bool {
    a.chars().last().is_some_and(is_katakana) && b.chars().next().is_some_and(is_katakana)
}

fn is_katakana(value: char) -> bool {
    matches!(value, '\u{30A0}'..='\u{30FF}' | '\u{31F0}'..='\u{31FF}' | '\u{FF66}'..='\u{FF9D}')
}

fn bibliography_continuation(a: &str, b: &str) -> bool {
    let last_line = a.lines().last().unwrap_or(a).trim();
    last_line.starts_with('*')
        && !b.starts_with('*')
        && !ends_with_terminal(a)
        && b.chars().count() <= 100
}

fn likely_visual_continuation(a: &ParagraphRecord, b: &ParagraphRecord) -> bool {
    let text_a = a.text.trim();
    let text_b = b.text.trim();
    if text_a.chars().count() < 10
        || ends_with_terminal(text_a)
        || looks_like_non_prose(text_a)
        || looks_like_non_prose(text_b)
        || a.format.is_list
        || b.format.is_list
        || a.format.is_heading
        || b.format.is_heading
        || a.format.style != b.format.style
    {
        return false;
    }
    continuation_format(a, b)
}

fn continuation_format(a: &ParagraphRecord, b: &ParagraphRecord) -> bool {
    let left_compatible = match (a.format.left_indent, b.format.left_indent) {
        (Some(left), Some(right)) => (left - right).abs() <= 8.0,
        _ => true,
    };
    if !left_compatible {
        return false;
    }
    let a_first = a.format.first_line_indent.unwrap_or(0.0);
    let b_first = b.format.first_line_indent.unwrap_or(0.0);
    (a_first >= 8.0 && b_first <= 2.0)
        || (a_first - b_first >= 6.0)
        || (a_first.abs() <= 2.0 && b_first.abs() <= 2.0)
}

fn ends_with_terminal(value: &str) -> bool {
    value
        .chars()
        .last()
        .is_some_and(|last| "。！？.!?」』）)]】〉》…：:;；".contains(last))
}

fn looks_like_non_prose(value: &str) -> bool {
    value.contains("http://")
        || value.contains("https://")
        || value.contains("www.")
        || value.matches('.').count() >= 8
        || value.starts_with('*')
        || value.chars().take(6).collect::<String>().contains('章')
        || value.lines().all(|line| {
            let trimmed = line.trim_start();
            trimmed
                .chars()
                .next()
                .is_some_and(|first| first.is_ascii_digit())
                && trimmed.contains('：')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_token_must_cross_the_boundary() {
        assert_eq!(
            verified_token_crossing_boundary("大学でプログラ", "ムされた"),
            Some("プログラム")
        );
        assert!(verified_token_crossing_boundary("プログラム", "された").is_none());
    }

    #[test]
    fn punctuation_and_units_are_mechanical() {
        assert!(is_detached_punctuation("。"));
        assert!(is_number_unit_split("それは100", "％正しい"));
    }
}
