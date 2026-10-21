//! The single shared safe-relative-path rule for user-supplied asset names.
//!
//! Game asset names use DOS/Windows backslash paths (e.g.
//! `sounds\car\ESCORT\FS-ESC.WAV`). Both the asset resolver and the media wave
//! source must map such a name to a safe relative path under a user-selected
//! root, rejecting anything that could escape it. This is the one place that
//! rule lives, so a hardening applies everywhere - previously the logic was
//! duplicated between `cmr-assets::resolver` and `cmr-core::audio_render`.

/// Validates `requested` as a safe relative asset path and returns it normalized
/// to forward-slash form, or `None` when it is empty, contains a NUL, is
/// absolute or UNC (leading `/` or `\`), is drive-qualified (`X:`...), ends with
/// a separator, or contains any empty, `.`, or `..` component.
#[must_use]
pub fn normalize_relative_path(requested: &str) -> Option<String> {
    if requested.is_empty()
        || requested.contains('\0')
        || requested.starts_with(['/', '\\'])
        || requested.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }

    let normalized = requested.replace('\\', "/");
    if normalized.ends_with('/')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_and_normalizes_backslash_paths() {
        assert_eq!(
            normalize_relative_path(r"sounds\car\ESCORT\FS-ESC.WAV").as_deref(),
            Some("sounds/car/ESCORT/FS-ESC.WAV")
        );
        assert_eq!(
            normalize_relative_path("a/b.wav").as_deref(),
            Some("a/b.wav")
        );
    }

    #[test]
    fn rejects_unsafe_shapes_including_windows_variants() {
        for unsafe_name in [
            "",
            "/etc/passwd",
            r"\windows\system",
            "C:\\x.wav",
            "C:x.wav",
            r"\\server\share\x.wav",
            "a/../b.wav",
            r"a\..\b.wav",
            "a/./b.wav",
            "a//b.wav",
            "dir/",
            "a\0b",
        ] {
            assert!(
                normalize_relative_path(unsafe_name).is_none(),
                "unsafe name was not rejected: {unsafe_name:?}"
            );
        }
    }
}
