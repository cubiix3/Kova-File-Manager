//! Snapshot-only search. Providers for future indexed search can reuse this query
//! without coupling the parser to a filesystem, UI toolkit, or Windows index.
use super::FileEntry;
use chrono::{DateTime, Datelike, Duration, Local};

#[derive(Clone, Debug, Default)]
pub struct SearchQuery {
    terms: Vec<String>,
    types: Vec<String>,
    extensions: Vec<String>,
    sizes: Vec<(char, u64)>,
    modified_after: Option<DateTime<Local>>,
}

impl SearchQuery {
    /// Unknown or malformed filters remain literal name terms; they must never
    /// silently broaden the results of a user's query.
    pub fn parse(input: &str, now: DateTime<Local>) -> Self {
        let mut query = Self::default();
        for token in input.split_whitespace().map(str::to_lowercase) {
            if let Some(kind) = token.strip_prefix("type:").filter(|s| !s.is_empty()) {
                query.types.push(kind.into());
            } else if let Some(ext) = token.strip_prefix("ext:").filter(|s| !s.is_empty()) {
                query.extensions.push(ext.trim_start_matches('.').into());
            } else if let Some(size) = token.strip_prefix("size:").and_then(parse_size) {
                query.sizes.push(size);
            } else if let Some(date) = token.strip_prefix("modified:").and_then(|value| {
                let date = match value {
                    "today" => now.date_naive(),
                    "this-week" => {
                        now.date_naive()
                            - Duration::days(now.weekday().num_days_from_monday().into())
                    }
                    "this-month" => now.date_naive().with_day(1)?,
                    _ => chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?,
                };
                date.and_hms_opt(0, 0, 0)?
                    .and_local_timezone(Local)
                    .earliest()
            }) {
                query.modified_after = Some(date);
            } else {
                query.terms.push(token);
            }
        }
        query
    }

    pub fn matches(&self, entry: &FileEntry) -> bool {
        self.matches_with_size(
            entry,
            entry.metadata.size.map(|bytes| super::EffectiveSize {
                bytes,
                complete: true,
            }),
        )
    }

    pub fn matches_with_size(&self, entry: &FileEntry, size: Option<super::EffectiveSize>) -> bool {
        let name = entry.name.to_lowercase();
        let ext = entry.extension_lower();
        self.terms.iter().all(|term| name.contains(term))
            && (self.extensions.is_empty() || self.extensions.contains(&ext))
            && (self.types.is_empty()
                || self.types.iter().any(|kind| match kind.as_str() {
                    "folder" | "directory" => entry.is_directory(),
                    "file" => !entry.is_directory(),
                    _ if entry.is_directory() => false,
                    "image" => {
                        !entry.is_directory()
                            && [
                                "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic",
                                "avif", "svg", "ico",
                            ]
                            .contains(&ext.as_str())
                    }
                    "video" => {
                        ["mp4", "mkv", "mov", "avi", "webm", "wmv", "m4v"].contains(&ext.as_str())
                    }
                    "audio" => {
                        ["mp3", "flac", "wav", "ogg", "m4a", "aac", "wma"].contains(&ext.as_str())
                    }
                    "document" => [
                        "pdf", "txt", "md", "doc", "docx", "odt", "xls", "xlsx", "ppt", "pptx",
                    ]
                    .contains(&ext.as_str()),
                    "archive" => {
                        ["zip", "7z", "rar", "tar", "gz", "xz", "bz2"].contains(&ext.as_str())
                    }
                    _ => ext == *kind,
                }))
            && self.sizes.iter().all(|(op, wanted)| {
                size.is_some_and(|size| match op {
                    '>' => size.bytes > *wanted,
                    '<' => size.complete && size.bytes < *wanted,
                    _ => size.complete && size.bytes == *wanted,
                })
            })
            && self
                .modified_after
                .is_none_or(|after| entry.metadata.modified.is_some_and(|date| date >= after))
    }
}

fn parse_size(value: &str) -> Option<(char, u64)> {
    let (op, value) = match value.as_bytes().first()? {
        b'>' => ('>', &value[1..]),
        b'<' => ('<', &value[1..]),
        b'=' => ('=', &value[1..]),
        _ => ('=', value),
    };
    let suffix = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let number: f64 = value[..suffix].parse().ok()?;
    let factor = match &value[suffix..] {
        "" | "b" => 1.0,
        "kb" | "kib" => 1024.0,
        "mb" | "mib" => 1024.0 * 1024.0,
        "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        "tb" | "tib" => 1024.0_f64.powi(4),
        _ => return None,
    };
    let bytes = number * factor;
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some((op, bytes as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{FileKind, FileMetadata};
    #[test]
    fn incomplete_folder_sizes_never_claim_an_upper_bound_or_exact_match() {
        let mut folder = entry();
        folder.kind = FileKind::Directory;
        folder.metadata.size = None;
        let size = Some(crate::domain::EffectiveSize {
            bytes: 2048,
            complete: false,
        });
        assert!(SearchQuery::parse("size:>1KiB", Local::now()).matches_with_size(&folder, size));
        assert!(!SearchQuery::parse("size:<3KiB", Local::now()).matches_with_size(&folder, size));
        assert!(!SearchQuery::parse("size:2KiB", Local::now()).matches_with_size(&folder, size));
    }
    fn entry() -> FileEntry {
        FileEntry {
            name: "Holiday.JPG".into(),
            path: "Holiday.JPG".into(),
            kind: FileKind::File,
            metadata: FileMetadata {
                size: Some(2 * 1024 * 1024),
                modified: Some(Local::now()),
                ..FileMetadata::empty()
            },
            icon_handle: None,
        }
    }
    #[test]
    fn combined_filters_and_case_insensitive_names() {
        assert!(
            SearchQuery::parse(
                "HOL type:image size:>1MB size:<3MB modified:today ext:jpg",
                Local::now()
            )
            .matches(&entry())
        );
        assert!(!SearchQuery::parse("type:video", Local::now()).matches(&entry()));
        assert!(!SearchQuery::parse("size:>2MB", Local::now()).matches(&entry()));
        assert!(!SearchQuery::parse("modified:invalid", Local::now()).matches(&entry()));
    }
    #[test]
    fn malformed_sizes_are_not_valid_filters() {
        for value in [">", "-1mb", "1XB", "NaN", "999999999999999999999999tb"] {
            assert!(parse_size(value).is_none(), "{value}");
        }
        assert_eq!(parse_size("1.5mb"), Some(('=', 1572864)));
    }
}
