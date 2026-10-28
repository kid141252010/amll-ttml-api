use super::token_matches;

#[test]
fn token_comparison() {
    assert!(token_matches("secret", "secret"));
    assert!(!token_matches("secret ", "secret"));
    assert!(!token_matches("", "secret"));
    assert!(!token_matches("SECRET", "secret"));
}
