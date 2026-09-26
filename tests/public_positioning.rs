//! Current product copy explains Exitbind from its own behavior. Supported
//! host documentation may be linked where setup needs it; research papers and
//! other products are not used as positioning or validation.

use std::fs;

const FIRST_USE: &[&str] = &[
    "README.md",
    "docs/onboarding.md",
    "docs/first-checked-run.md",
    "docs/cross-host-continuation.md",
    "skills/exitbind/SKILL.md",
    "plugins/exitbind/skills/exitbind/SKILL.md",
];
const RESEARCH_HOSTS: &[&str] = &[
    "arxiv.org",
    "doi.org",
    "openreview.net",
    "semanticscholar.org",
];
const README_LINK_PREFIXES: &[&str] = &[
    "https://github.com/envy404/exitbind",
    "https://raw.githubusercontent.com/envy404/exitbind/",
    "https://img.shields.io/",
];

fn links(text: &str) -> Vec<&str> {
    text.match_indices("http")
        .filter_map(|(start, _)| {
            let rest = &text[start..];
            (rest.starts_with("https://") || rest.starts_with("http://")).then(|| {
                let end = rest
                    .find(|c: char| c.is_whitespace() || matches!(c, ')' | '"' | '\'' | '>' | '`'))
                    .unwrap_or(rest.len());
                &rest[..end]
            })
        })
        .collect()
}

#[test]
fn first_use_surfaces_cite_no_research_papers() {
    let root = env!("CARGO_MANIFEST_DIR");
    for path in FIRST_USE {
        let text = fs::read_to_string(format!("{root}/{path}")).unwrap();
        for link in links(&text) {
            assert!(
                !RESEARCH_HOSTS.iter().any(|host| link.contains(host)),
                "{path} cites {link}"
            );
        }
    }
}

#[test]
fn readme_links_only_exitbind_owned_or_badge_locations() {
    let text = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/README.md")).unwrap();
    for link in links(&text) {
        assert!(
            README_LINK_PREFIXES
                .iter()
                .any(|prefix| link.starts_with(prefix)),
            "README links outside Exitbind: {link}"
        );
    }
}
