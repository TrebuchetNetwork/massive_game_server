pub const MAX_MODEL_ID_LEN: usize = 128;

pub fn sanitize_model_id(model_id: &str) -> Option<String> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_MODEL_ID_LEN {
        return None;
    }
    if trimmed.starts_with('.') || trimmed.ends_with('.') || trimmed.contains("..") {
        return None;
    }
    if trimmed
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-' || ch == b'.')
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_model_id_rejects_traversal_and_hidden() {
        assert!(sanitize_model_id("../etc/passwd").is_none());
        assert!(sanitize_model_id(".hidden-model").is_none());
        assert!(sanitize_model_id("model..double-dot").is_none());
        assert!(sanitize_model_id("model.").is_none());
    }

    #[test]
    fn sanitize_model_id_accepts_expected_characters() {
        assert_eq!(
            sanitize_model_id("bot-alpha_1.2"),
            Some("bot-alpha_1.2".to_owned())
        );
    }

    #[test]
    fn sanitize_model_id_enforces_max_length() {
        assert!(sanitize_model_id(&"a".repeat(MAX_MODEL_ID_LEN + 1)).is_none());
    }
}
