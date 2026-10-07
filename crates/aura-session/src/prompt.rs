use aura_core::transcript::Speaker;

/// `prompts/realtime/listener.md`, without its front matter.
pub fn listener(speaker: Speaker) -> String {
    const SOURCE: &str = include_str!("../../../prompts/realtime/listener.md");
    let body = SOURCE
        .strip_prefix("---")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map_or(SOURCE, |(_, body)| body);
    let role = match speaker {
        Speaker::Seller => "the seller's microphone: one person speaking to their customers",
        Speaker::Customer => "the meeting audio: the customer and other participants",
    };
    body.trim().replace("{{role}}", role)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_stripped_and_the_role_is_filled_in() {
        for speaker in [Speaker::Seller, Speaker::Customer] {
            let prompt = listener(speaker);
            assert!(prompt.starts_with("You are a silent observer"), "{prompt}");
            assert!(!prompt.contains("{{"));
            assert!(!prompt.contains("version:"));
        }
        assert!(listener(Speaker::Seller).contains("microphone"));
        assert!(listener(Speaker::Customer).contains("customer and other participants"));
    }
}
