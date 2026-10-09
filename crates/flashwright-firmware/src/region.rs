// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Clatty Works

/// LU0 and FIPS packages are refused before any image is extracted.
pub(crate) fn is_restricted_region(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for token in [b"lu0".as_slice(), b"fips".as_slice()] {
        let mut start = 0;
        while let Some(rel) = lower[start..].find(std::str::from_utf8(token).unwrap()) {
            let index = start + rel;
            let before_ok = index == 0 || !bytes[index - 1].is_ascii_alphanumeric();
            let after = index + token.len();
            let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return true;
            }
            start = index + token.len();
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_whole_words() {
        assert!(is_restricted_region("komodo-lu0-factory"));
        assert!(is_restricted_region("region=FIPS"));
        assert!(is_restricted_region("board=komodo_fips"));
        assert!(!is_restricted_region("fingerprints"));
        assert!(!is_restricted_region("komodo"));
    }
}
