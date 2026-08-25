use crate::docx::ParagraphRecord;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Confidence {
    Certain,
    Review,
}

pub(crate) struct RuleMatch {
    pub(crate) confidence: Confidence,
    pub(crate) suggested_merge: bool,
    pub(crate) code: &'static str,
    pub(crate) reason: &'static str,
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
            code: "detached_punctuation",
            reason:
                "The next paragraph contains only punctuation detached from the preceding text.",
        });
    }
    if is_number_unit_split(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            code: "number_unit_split",
            reason: "A number has been separated from its unit.",
        });
    }
    if let Some(token) = verified_token_crossing_boundary(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            code: "verified_token_split",
            reason: token_reason(token),
        });
    }
    if katakana_crosses_boundary(a, b) && continuation_format(previous, following) {
        return Some(RuleMatch {
            confidence: Confidence::Certain,
            suggested_merge: true,
            code: "katakana_token_split",
            reason: "A continuous Katakana token is split across two Word paragraphs.",
        });
    }

    if bibliography_continuation(a, b) {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: true,
            code: "bibliography_continuation",
            reason: "The next paragraph appears to continue the same starred title.",
        });
    }

    if quoted_clause_continuation(previous, following) {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: false,
            code: "quoted_clause_continuation",
            reason:
                "A clause without terminal punctuation appears to continue into a quoted phrase.",
        });
    }

    if likely_visual_continuation(previous, following) {
        return Some(RuleMatch {
            confidence: Confidence::Review,
            suggested_merge: true,
            code: "visual_continuation",
            reason: "Paragraph formatting and incomplete text make this look like a hidden continuation.",
        });
    }
    None
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
