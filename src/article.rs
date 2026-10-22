#[derive(Debug, PartialEq, Eq)]
pub enum Part {
    Translate(String),
    Whitespace(String),
}
pub fn translate_with(
    text: &str,
    mut translate: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    let mut result = String::new();
    for part in split(text, 1500) {
        match part {
            Part::Whitespace(value) => result.push_str(&value),
            Part::Translate(value) => result.push_str(&translate(&value)?),
        }
    }
    Ok(result)
}
pub fn split(text: &str, limit: usize) -> Vec<Part> {
    let limit = limit.max(1);
    split_at(text, false, |paragraph| {
        paragraph
            .char_indices()
            .nth(limit)
            .map(|(i, _)| i)
            .unwrap_or(paragraph.len())
    })
}
/// Bound web requests conservatively; this is a chunk size, not a usage quota.
pub fn split_online(text: &str) -> Vec<Part> {
    split_at(text, true, |paragraph| {
        let mut end = paragraph.len().min(500);
        while !paragraph.is_char_boundary(end) {
            end -= 1;
        }
        end
    })
}

/// Keep short LLM requests together so each request can use the full context.
/// Long inputs are split only at line boundaries until the configured limit.
pub fn split_llm(text: &str, limit: usize) -> Vec<Part> {
    let limit = limit.max(1);
    if text.chars().count() <= limit {
        return vec![Part::Translate(text.into())];
    }
    let mut parts = Vec::new();
    let mut current = String::new();
    for line in text.split_inclusive('\n') {
        if !current.is_empty() && current.chars().count() + line.chars().count() > limit {
            parts.push(Part::Translate(std::mem::take(&mut current)));
        }
        if line.chars().count() > limit {
            if !current.is_empty() {
                parts.push(Part::Translate(std::mem::take(&mut current)));
            }
            parts.extend(split(line, limit));
        } else {
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        parts.push(Part::Translate(current));
    }
    parts
}
fn split_at(text: &str, prefer_sentence: bool, hard_boundary: impl Fn(&str) -> usize) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let leading = rest.len() - rest.trim_start().len();
        if leading > 0 {
            let value = &rest[..leading];
            if let Some(Part::Whitespace(previous)) = parts.last_mut() {
                previous.push_str(value);
            } else {
                parts.push(Part::Whitespace(value.into()));
            }
            rest = &rest[leading..];
            continue;
        }
        let paragraph_end = rest.find(['\n', '\r']).unwrap_or(rest.len());
        let paragraph = &rest[..paragraph_end];
        let hard_end = hard_boundary(paragraph);
        let mut end = hard_end;
        let sentence_end = if prefer_sentence && hard_end < paragraph.len() {
            paragraph[..hard_end]
                .char_indices()
                .rev()
                .find_map(|(i, c)| {
                    let next = i + c.len_utf8();
                    let boundary = matches!(c, '。' | '！' | '？')
                        || (matches!(c, '.' | '!' | '?')
                            && paragraph[next..].starts_with(char::is_whitespace));
                    boundary.then_some(next)
                })
        } else {
            None
        };
        if let Some(sentence_end) = sentence_end {
            end = sentence_end;
        } else if hard_end < paragraph.len()
            && let Some((i, _)) = paragraph[..hard_end].char_indices().rev().find(|(_, c)| {
                c.is_whitespace() || matches!(c, '。' | '！' | '？' | '.' | '!' | '?')
            })
            && i > 0
        {
            end = if paragraph[i..].starts_with(char::is_whitespace) {
                i
            } else {
                i + paragraph[i..].chars().next().unwrap().len_utf8()
            };
        }
        let trimmed_end = paragraph[..end].trim_end().len();
        parts.push(Part::Translate(rest[..trimmed_end].into()));
        rest = &rest[trimmed_end..];
    }
    parts
}
