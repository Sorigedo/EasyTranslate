fn numbers(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || (!current.is_empty() && matches!(c, ',' | '.')) {
            current.push(c);
        } else if !current.is_empty() {
            tokens.push(current.trim_end_matches([',', '.']).replace(',', ""));
            current.clear();
        }
    }
    tokens.sort();
    tokens
}
/// A conservative numeric consistency hint, not a semantic correctness score.
pub fn number_warning(source: &str, translation: &str) -> Option<String> {
    (numbers(source) != numbers(translation))
        .then(|| "原文与译文中的数字不一致，请核对金额、日期和数量。".into())
}
