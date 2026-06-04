//! Token counting utilities — 1 token ≈ 3 chars (DeepSeek official ratio)

use crate::types::message::Message;

/// Estimate token count from character length (DeepSeek: 1 token ≈ 3 English chars)
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / 3
}

/// Estimate tokens for a batch of messages
pub fn estimate_message_tokens(messages: &[Message]) -> usize {
    messages.iter().map(|m| m.estimated_char_len()).sum::<usize>() / 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_short_text() {
        assert_eq!(estimate_tokens("hello"), 1);
    }

    #[test]
    fn test_divisible_by_3() {
        assert_eq!(estimate_tokens("abcdef"), 2);
    }
}
