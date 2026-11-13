use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn load(file: &Path) -> io::Result<Vec<PathBuf>> {
    let file = std::path::absolute(file)?;
    let folder = file
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Select a playlist file."))?;
    let content = fs::read_to_string(&file)?;
    Ok(content
        .trim_start_matches('\u{feff}')
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| folder.join(line.strip_prefix("./").unwrap_or(line)))
        .collect())
}

pub fn save(file: &Path, tracks: &[PathBuf]) -> io::Result<()> {
    let file = std::path::absolute(file)?;
    let folder = file
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Select a playlist file."))?;
    let mut content = String::from("#EXTM3U\n");
    for track in tracks {
        let absolute = std::path::absolute(track)?;
        let path = absolute.strip_prefix(folder).unwrap_or(&absolute);
        let value = path
            .to_str()
            .filter(|value| !value.contains(['\r', '\n']))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "An audio path cannot be written as a single UTF-8 line.",
                )
            })?;
        if value.trim_start().starts_with('#') {
            content.push_str("./");
        }
        content.push_str(value);
        content.push('\n');
    }
    fs::write(file, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_order_duplicates_and_unicode_paths() -> io::Result<()> {
        let folder =
            std::env::temp_dir().join(format!("rust-radio-playlist-{}", std::process::id()));
        fs::create_dir(&folder)?;
        let file = folder.join("show.m3u8");
        let track = folder.join("# Grüne Welle.flac");
        let tracks = vec![
            track.clone(),
            folder.join("music").join("second.mp3"),
            track,
        ];
        let result = save(&file, &tracks).and_then(|_| load(&file));
        fs::remove_file(&file)?;
        fs::remove_dir(&folder)?;
        assert_eq!(result?, tracks);
        Ok(())
    }
}
