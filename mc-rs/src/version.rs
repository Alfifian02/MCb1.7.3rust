//! Which build this is. `.github/workflows/ci.yml` builds the APK with `MCRS_BUILD` (the run number) and `MCRS_SHA` (the commit) set, so the label
//! is the tag of the GitHub release the APK came from (`build-<n>`) and the commit it was built at. A build made anywhere else has neither and
//! reads `dev`. Shown on the title screen and the debug screen. Nothing here touches the network (an update check is a separate, later feature).

/// The CI's run number, read at compile time (`None` for a local build).
const BUILD: Option<&str> = option_env!("MCRS_BUILD");
/// The commit the CI built, in full (`None` for a local build).
const SHA: Option<&str> = option_env!("MCRS_SHA");

/// `build-123 (a1b2c3d)`, `build-123` without a commit, `dev` without a run number (an empty value counts as missing).
fn label_of(build: Option<&str>, sha: Option<&str>) -> String {
    let Some(n) = build.map(str::trim).filter(|s| !s.is_empty()) else { return "dev".to_string() };
    match sha.map(str::trim).filter(|s| !s.is_empty()) {
        Some(c) => format!("build-{n} ({})", c.chars().take(7).collect::<String>()),
        None => format!("build-{n}"),
    }
}

/// This build's label.
pub fn label() -> String {
    label_of(BUILD, SHA)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The run number and the first 7 characters of the commit; a missing or empty run number is a local build.
    #[test]
    fn label_names_the_release_and_the_commit() {
        assert_eq!(label_of(Some("123"), Some("a1b2c3d4e5f60718293a4b5c6d7e8f9012345678")), "build-123 (a1b2c3d)");
        assert_eq!(label_of(Some("7"), Some("abc")), "build-7 (abc)");
        assert_eq!(label_of(Some("123"), None), "build-123");
        assert_eq!(label_of(Some("123"), Some("")), "build-123");
        assert_eq!(label_of(None, Some("a1b2c3d")), "dev");
        assert_eq!(label_of(Some(" "), Some("a1b2c3d")), "dev");
        assert!(!label().is_empty());
    }
}
