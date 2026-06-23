#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    Single,
    Double,
}

impl QuoteKind {
    fn from_char(ch: char) -> Option<Self> {
        match ch {
            '\'' => Some(Self::Single),
            '"' => Some(Self::Double),
            _ => None,
        }
    }

    fn as_char(self) -> char {
        match self {
            Self::Single => '\'',
            Self::Double => '"',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiteralAwareChar {
    pub byte_idx: usize,
    pub ch: char,
    pub inside_literal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpannedWord {
    pub text: String,
    pub start: usize,
    pub end: usize,
}

pub struct LiteralAwareCharIndices<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    quote: Option<QuoteKind>,
    pending: Option<LiteralAwareChar>,
}

impl<'a> LiteralAwareCharIndices<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.char_indices().peekable(),
            quote: None,
            pending: None,
        }
    }
}

impl Iterator for LiteralAwareCharIndices<'_> {
    type Item = LiteralAwareChar;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(pending) = self.pending.take() {
            return Some(pending);
        }

        let (byte_idx, ch) = self.chars.next()?;
        if let Some(quote) = self.quote {
            if ch == quote.as_char() {
                if let Some((next_idx, next_ch)) = self.chars.peek().copied() {
                    if next_ch == ch {
                        self.chars.next();
                        self.pending = Some(LiteralAwareChar {
                            byte_idx: next_idx,
                            ch: next_ch,
                            inside_literal: true,
                        });
                        return Some(LiteralAwareChar {
                            byte_idx,
                            ch,
                            inside_literal: true,
                        });
                    }
                }
                self.quote = None;
            }
            return Some(LiteralAwareChar {
                byte_idx,
                ch,
                inside_literal: true,
            });
        }

        if let Some(quote) = QuoteKind::from_char(ch) {
            self.quote = Some(quote);
            return Some(LiteralAwareChar {
                byte_idx,
                ch,
                inside_literal: true,
            });
        }

        Some(LiteralAwareChar {
            byte_idx,
            ch,
            inside_literal: false,
        })
    }
}

pub fn literal_aware_char_indices(text: &str) -> LiteralAwareCharIndices<'_> {
    LiteralAwareCharIndices::new(text)
}

pub fn quoted_literal_end(text: &str, start: usize) -> Option<usize> {
    let quote = text.get(start..)?.chars().next()?;
    QuoteKind::from_char(quote)?;
    Some(complete_quoted_literal_end(text, start).unwrap_or(text.len()))
}

pub fn complete_quoted_literal_end(text: &str, start: usize) -> Option<usize> {
    let quote = text.get(start..)?.chars().next()?;
    QuoteKind::from_char(quote)?;
    let mut idx = start + quote.len_utf8();
    while idx < text.len() {
        let ch = text[idx..].chars().next()?;
        let next_idx = idx + ch.len_utf8();
        if ch == quote {
            if text[next_idx..].starts_with(quote) {
                idx = next_idx + quote.len_utf8();
                continue;
            }
            return Some(next_idx);
        }
        idx = next_idx;
    }
    None
}

pub fn split_cobol_words_spanned(raw: &str) -> Vec<SpannedWord> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_start = None::<usize>;
    let mut current_end = 0usize;
    let mut idx = 0usize;
    while idx < raw.len() {
        let Some(ch) = raw[idx..].chars().next() else {
            break;
        };
        if QuoteKind::from_char(ch).is_some() {
            if let Some(end) = quoted_literal_end(raw, idx) {
                current_start.get_or_insert(idx);
                current.push_str(&raw[idx..end]);
                current_end = end;
                idx = end;
                continue;
            }
        }
        if raw[idx..].starts_with("==") {
            if let Some(end) = pseudo_text_end(raw, idx) {
                current_start.get_or_insert(idx);
                current.push_str(&raw[idx..end]);
                current_end = end;
                idx = end;
                continue;
            }
            current_start.get_or_insert(idx);
            current.push_str(&raw[idx..]);
            current_end = raw.len();
            break;
        }
        if is_word_separator(raw, idx, ch) {
            if !current.is_empty() {
                out.push(SpannedWord {
                    text: std::mem::take(&mut current),
                    start: current_start.expect("token start"),
                    end: current_end,
                });
                current_start = None;
            }
        } else {
            current_start.get_or_insert(idx);
            current.push(ch);
            current_end = idx + ch.len_utf8();
        }
        idx += ch.len_utf8();
    }
    if !current.is_empty() {
        out.push(SpannedWord {
            text: current,
            start: current_start.expect("token start"),
            end: current_end,
        });
    }
    out
}

fn is_word_separator(raw: &str, idx: usize, ch: char) -> bool {
    if matches!(ch, ' ' | '\t' | '\r' | '\n') {
        return true;
    }
    if matches!(ch, ',' | ';') {
        let next_idx = idx + ch.len_utf8();
        return raw[next_idx..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace);
    }
    false
}

pub fn split_cobol_words(raw: &str) -> Vec<String> {
    split_cobol_words_spanned(raw)
        .into_iter()
        .map(|word| word.text)
        .collect()
}

pub fn replace_outside_literals_case_insensitive<F>(
    text: &str,
    from: &str,
    to: &str,
    is_boundary: F,
) -> String
where
    F: Fn(&str, usize) -> bool,
{
    if from.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;
    while idx < text.len() {
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if text[idx..].starts_with("==") {
            if let Some(end) = pseudo_text_end(text, idx) {
                out.push_str(&text[idx..end]);
                idx = end;
                continue;
            }
            out.push_str(&text[idx..]);
            break;
        }
        if QuoteKind::from_char(ch).is_some() {
            if let Some(end) = quoted_literal_end(text, idx) {
                out.push_str(&text[idx..end]);
                idx = end;
                continue;
            }
        }
        if starts_with_ignore_ascii_case(&text[idx..], from)
            && is_boundary(text, idx)
            && is_boundary(text, idx + from.len())
        {
            out.push_str(to);
            idx += from.len();
        } else {
            out.push(ch);
            idx += ch.len_utf8();
        }
    }
    out
}

pub fn strip_inline_comment_outside_literals(text: &str) -> &str {
    let mut idx = 0usize;
    let mut unmatched_quote_seen = false;
    while idx < text.len() {
        if !unmatched_quote_seen && text[idx..].starts_with("==") {
            if let Some(end) = pseudo_text_end(text, idx) {
                idx = end;
                continue;
            }
        }
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = complete_quoted_literal_end(text, idx) {
                idx = end;
                continue;
            }
            unmatched_quote_seen = true;
            idx += ch.len_utf8();
            continue;
        }
        if text[idx..].starts_with("*>") {
            return &text[..idx];
        }
        idx += ch.len_utf8();
    }
    text
}

pub fn pseudo_text_end(text: &str, start: usize) -> Option<usize> {
    if !text.get(start..)?.starts_with("==") {
        return None;
    }
    let mut idx = start + 2;
    let mut quote = None;
    while idx < text.len() {
        let ch = text[idx..].chars().next()?;
        if let Some(active_quote) = quote {
            if ch == active_quote {
                let next_idx = idx + ch.len_utf8();
                if text[next_idx..].starts_with(active_quote) {
                    idx = next_idx + active_quote.len_utf8();
                    continue;
                }
                quote = None;
            }
            idx += ch.len_utf8();
            continue;
        }
        if QuoteKind::from_char(ch).is_some() {
            quote = Some(ch);
            idx += ch.len_utf8();
            continue;
        }
        if text[idx..].starts_with("==") {
            return Some(idx + 2);
        }
        idx += ch.len_utf8();
    }
    None
}

pub fn strip_trailing_sentence_period_outside_literals(text: &str) -> &str {
    let trimmed = text.trim_end();
    let Some(period_idx) = trimmed.rfind('.') else {
        return trimmed;
    };
    if !is_sentence_period_outside_literals(trimmed, period_idx) {
        return trimmed;
    }
    trimmed[..period_idx].trim_end()
}

pub fn is_sentence_period_outside_literals(text: &str, period_idx: usize) -> bool {
    if !is_period_outside_literals(text, period_idx) {
        return false;
    }
    let next_idx = period_idx + '.'.len_utf8();
    let followed_by_boundary = text
        .get(next_idx..)
        .and_then(|tail| tail.chars().next())
        .map(|ch| ch.is_whitespace())
        .unwrap_or(true);
    if !followed_by_boundary {
        return false;
    }
    true
}

pub fn is_period_outside_literals(text: &str, period_idx: usize) -> bool {
    if !text
        .get(period_idx..)
        .and_then(|tail| tail.chars().next())
        .is_some_and(|ch| ch == '.')
    {
        return false;
    }
    let mut idx = 0usize;
    while idx < text.len() {
        if text[idx..].starts_with("==") {
            if let Some(end) = pseudo_text_end(text, idx) {
                if period_idx < end {
                    return false;
                }
                idx = end;
                continue;
            }
            return false;
        }
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = quoted_literal_end(text, idx) {
                if period_idx < end {
                    return false;
                }
                idx = end;
                continue;
            }
        }
        if idx == period_idx {
            return true;
        }
        idx += ch.len_utf8();
    }
    false
}

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .map(|head| head.eq_ignore_ascii_case(prefix))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_literal_end_handles_doubled_quotes() {
        assert_eq!(quoted_literal_end("\"A\"\"B\".", 0), Some(6));
        assert_eq!(quoted_literal_end("'A''B'.", 0), Some(6));
    }

    #[test]
    fn complete_quoted_literal_end_rejects_unclosed_literals() {
        assert_eq!(complete_quoted_literal_end("\"A\"\"B\".", 0), Some(6));
        assert_eq!(complete_quoted_literal_end("\"A", 0), None);
        assert_eq!(complete_quoted_literal_end("\"\"\"", 0), None);
        assert_eq!(quoted_literal_end("\"A", 0), Some(2));
    }

    #[test]
    fn literal_aware_chars_keep_escaped_quote_pair_inside_literal() {
        let period = literal_aware_char_indices("\"A\"\".B\".")
            .find(|item| item.ch == '.')
            .expect("period");
        assert!(period.inside_literal);
        let final_period = literal_aware_char_indices("\"A\"\".B\".")
            .filter(|item| item.ch == '.')
            .last()
            .expect("final period");
        assert!(!final_period.inside_literal);
    }

    #[test]
    fn literal_aware_chars_keep_single_quote_period_inside_literal() {
        let periods = literal_aware_char_indices("'CAN''T.STOP'.")
            .filter(|item| item.ch == '.')
            .map(|item| item.inside_literal)
            .collect::<Vec<_>>();
        assert_eq!(periods, vec![true, false]);
    }

    #[test]
    fn split_words_preserves_doubled_quote_literal() {
        assert_eq!(
            split_cobol_words("DISPLAY \"A\"\"B\", C"),
            vec!["DISPLAY", "\"A\"\"B\"", "C"]
        );
    }

    #[test]
    fn split_words_preserves_doubled_single_quote_literal() {
        assert_eq!(
            split_cobol_words("DISPLAY 'CAN''T, STOP', WS-FLAG"),
            vec!["DISPLAY", "'CAN''T, STOP'", "WS-FLAG"]
        );
    }

    #[test]
    fn split_words_preserves_complete_pseudo_text() {
        assert_eq!(
            split_cobol_words("COPY REC REPLACING ==OLD FIELD== BY ==NEW FIELD=="),
            vec![
                "COPY",
                "REC",
                "REPLACING",
                "==OLD FIELD==",
                "BY",
                "==NEW FIELD=="
            ]
        );
    }

    #[test]
    fn split_words_preserves_unclosed_pseudo_text() {
        assert_eq!(
            split_cobol_words("COPY REC REPLACING ==OLD FIELD BY NEW FIELD"),
            vec!["COPY", "REC", "REPLACING", "==OLD FIELD BY NEW FIELD"]
        );
    }

    #[test]
    fn split_words_treats_semicolon_separator_like_space() {
        assert_eq!(
            split_cobol_words("COPY REC; REPLACING ==OLD== BY ==NEW=="),
            vec!["COPY", "REC", "REPLACING", "==OLD==", "BY", "==NEW=="]
        );
    }

    #[test]
    fn split_words_keeps_nonseparator_punctuation_in_token() {
        assert_eq!(
            split_cobol_words("COPY REC,ALT REC;ALT"),
            vec!["COPY", "REC,ALT", "REC;ALT"]
        );
    }

    #[test]
    fn spanned_words_slice_back_to_exact_token_text() {
        let raw = "READ F AT END DISPLAY \"A.B\" DISPLAY \"A\"\"B\", WS-FLAG";
        let words = split_cobol_words_spanned(raw);
        assert_eq!(
            words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            vec![
                "READ",
                "F",
                "AT",
                "END",
                "DISPLAY",
                "\"A.B\"",
                "DISPLAY",
                "\"A\"\"B\"",
                "WS-FLAG",
            ]
        );
        for word in &words {
            assert_eq!(&raw[word.start..word.end], word.text);
        }
    }

    #[test]
    fn legacy_split_words_delegates_to_spanned_words() {
        let raw = "DISPLAY 'CAN''T, STOP', \"A\"\"B\".";
        let spanned = split_cobol_words_spanned(raw)
            .into_iter()
            .map(|word| word.text)
            .collect::<Vec<_>>();
        assert_eq!(split_cobol_words(raw), spanned);
    }

    #[test]
    fn replace_outside_literals_skips_single_and_double_literals() {
        let is_boundary = |text: &str, idx: usize| {
            if idx == 0 || idx >= text.len() {
                return true;
            }
            let before = text[..idx].chars().next_back();
            let after = text[idx..].chars().next();
            before.map_or(true, |ch| !ch.is_ascii_alphanumeric())
                || after.map_or(true, |ch| !ch.is_ascii_alphanumeric())
        };
        let replaced = replace_outside_literals_case_insensitive(
            "DISPLAY 'OLD''X' \"OLD\" OLD.",
            "OLD",
            "NEW",
            is_boundary,
        );
        assert_eq!(replaced, "DISPLAY 'OLD''X' \"OLD\" NEW.");
    }

    #[test]
    fn replace_outside_literals_skips_complete_pseudo_text() {
        let is_boundary = |text: &str, idx: usize| {
            if idx == 0 || idx >= text.len() {
                return true;
            }
            let before = text[..idx].chars().next_back();
            let after = text[idx..].chars().next();
            before.map_or(true, |ch| !ch.is_ascii_alphanumeric())
                || after.map_or(true, |ch| !ch.is_ascii_alphanumeric())
        };
        let replaced = replace_outside_literals_case_insensitive(
            "COPY REC REPLACING ==OLD TOKEN== BY ==OLD== OLD.",
            "OLD",
            "NEW",
            is_boundary,
        );
        assert_eq!(replaced, "COPY REC REPLACING ==OLD TOKEN== BY ==OLD== NEW.");
    }

    #[test]
    fn replace_outside_literals_preserves_unclosed_pseudo_text() {
        let is_boundary = |text: &str, idx: usize| {
            if idx == 0 || idx >= text.len() {
                return true;
            }
            let before = text[..idx].chars().next_back();
            let after = text[idx..].chars().next();
            before.map_or(true, |ch| !ch.is_ascii_alphanumeric())
                || after.map_or(true, |ch| !ch.is_ascii_alphanumeric())
        };
        let replaced = replace_outside_literals_case_insensitive(
            "COPY REC REPLACING ==OLD TOKEN OLD.",
            "OLD",
            "NEW",
            is_boundary,
        );
        assert_eq!(replaced, "COPY REC REPLACING ==OLD TOKEN OLD.");
    }

    #[test]
    fn inline_comment_detection_ignores_comment_marker_inside_literals() {
        assert_eq!(
            strip_inline_comment_outside_literals("DISPLAY \"A *> B\" *> real comment"),
            "DISPLAY \"A *> B\" "
        );
        assert_eq!(
            strip_inline_comment_outside_literals("DISPLAY 'CAN''T *> STOP' *> real comment"),
            "DISPLAY 'CAN''T *> STOP' "
        );
    }

    #[test]
    fn inline_comment_detection_ignores_comment_marker_inside_pseudo_text() {
        assert_eq!(
            strip_inline_comment_outside_literals(
                "COPY REC REPLACING ==OLD *> TOKEN== BY ==NEW==. *> real comment"
            ),
            "COPY REC REPLACING ==OLD *> TOKEN== BY ==NEW==. "
        );
    }

    #[test]
    fn inline_comment_detection_strips_comment_after_unclosed_pseudo_text_marker() {
        assert_eq!(
            strip_inline_comment_outside_literals("DISPLAY == *> real comment"),
            "DISPLAY == "
        );
    }

    #[test]
    fn inline_comment_detection_strips_comment_after_unclosed_quote() {
        assert_eq!(
            strip_inline_comment_outside_literals("DISPLAY \"A *> real comment"),
            "DISPLAY \"A "
        );
    }

    #[test]
    fn inline_comment_detection_does_not_let_pseudo_text_after_unclosed_quote_hide_comment() {
        assert_eq!(
            strip_inline_comment_outside_literals("DISPLAY \"A ==OLD *> TOKEN== *> comment"),
            "DISPLAY \"A ==OLD "
        );
    }

    #[test]
    fn trailing_sentence_period_strip_is_literal_aware() {
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("COPY REC."),
            "COPY REC"
        );
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("DISPLAY \"A.\"."),
            "DISPLAY \"A.\""
        );
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("DISPLAY \"A.\""),
            "DISPLAY \"A.\""
        );
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("DISPLAY \"A\"\".\"."),
            "DISPLAY \"A\"\".\""
        );
    }

    #[test]
    fn trailing_sentence_period_strip_preserves_period_inside_pseudo_text() {
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("REPLACE ==END== BY ==. =="),
            "REPLACE ==END== BY ==. =="
        );
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("REPLACE ==END== BY ==. ==."),
            "REPLACE ==END== BY ==. =="
        );
    }

    #[test]
    fn trailing_sentence_period_strip_preserves_period_inside_unclosed_pseudo_text() {
        assert_eq!(
            strip_trailing_sentence_period_outside_literals("DISPLAY == A."),
            "DISPLAY == A."
        );
    }

    #[test]
    fn sentence_period_predicate_is_literal_aware() {
        assert!(is_sentence_period_outside_literals("COPY REC.", 8));
        assert!(is_sentence_period_outside_literals("COPY REC. NEXT", 8));
        assert!(!is_sentence_period_outside_literals("MOVE 12.34 TO X.", 7));
        assert!(!is_sentence_period_outside_literals("DISPLAY \"A.B\".", 10));
        assert!(is_sentence_period_outside_literals("DISPLAY \"A.B\".", 13));
    }

    #[test]
    fn period_predicate_is_literal_aware_without_requiring_sentence_boundary() {
        assert!(is_period_outside_literals("COPY REC.COPY NEXT.", 8));
        assert!(!is_period_outside_literals("DISPLAY \"A.B\".", 10));
        assert!(is_period_outside_literals("DISPLAY \"A.B\".", 13));
    }
}
