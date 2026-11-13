use std::{fs::File, path::Path};

use symphonia::core::{
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::{MetadataOptions, MetadataRevision, StandardTagKey},
    probe::Hint,
};

pub fn track_title(path: &Path) -> String {
    read_tags(path).unwrap_or_else(|| crate::track_name(path))
}

fn read_tags(path: &Path) -> Option<String> {
    let source = MediaSourceStream::new(
        Box::new(File::open(path).ok()?),
        MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let mut probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    probed
        .format
        .metadata()
        .current()
        .and_then(display_tags)
        .or_else(|| probed.metadata.get()?.current().and_then(display_tags))
}

fn display_tags(revision: &MetadataRevision) -> Option<String> {
    let tag = |key| {
        revision
            .tags()
            .iter()
            .find(|tag| tag.std_key == Some(key))
            .map(|tag| tag.value.to_string())
            .filter(|text| !text.trim().is_empty())
    };
    let title = tag(StandardTagKey::TrackTitle)?;
    Some(match tag(StandardTagKey::Artist) {
        Some(artist) => format!("{artist} - {title}"),
        None => title,
    })
}

pub fn url_encode(text: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}
