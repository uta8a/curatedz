use curatedz_core::article_fetch::robots_allowed;

#[test]
fn robots_uses_longest_matching_rule() {
    let robots = "User-agent: *\nUser-agent: curatedz\nDisallow: /private\nAllow: /private/public\nUser-agent: other\nDisallow: /news\n";
    assert!(!robots_allowed(robots, "/private/secret"));
    assert!(robots_allowed(robots, "/private/public/article"));
    assert!(robots_allowed(robots, "/news"));
}
