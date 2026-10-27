//! Release identity is injected at build time; Cargo keeps the base version.

pub fn release_version() -> &'static str {
    option_env!("DISH_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

pub fn is_local_build() -> bool {
    option_env!("DISH_RELEASE_VERSION").is_none()
        || release_version().split(['-', '.', '+']).any(|part| part == "local")
}

pub fn display_version() -> String {
    if is_local_build() {
        format!("{} (build local)", release_version())
    } else {
        release_version().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_identity_uses_build_label_or_cargo_fallback() {
        assert_eq!(release_version(), option_env!("DISH_RELEASE_VERSION")
            .unwrap_or(env!("CARGO_PKG_VERSION")));
        assert_eq!(display_version().ends_with(" (build local)"), is_local_build());
    }
}
