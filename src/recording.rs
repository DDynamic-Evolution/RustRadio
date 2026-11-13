use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::Stdio,
    sync::mpsc::{self, SyncSender},
    thread::{self, JoinHandle},
};

use crate::broadcast::{Pcm, mp3_encoder};

pub struct Recording {
    pub path: PathBuf,
    worker: JoinHandle<io::Result<()>>,
}

impl Recording {
    pub fn start(folder: &Path) -> io::Result<(Self, SyncSender<Pcm>)> {
        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let mut sequence = 0;
        let (path, file) = loop {
            let name = if sequence == 0 {
                format!("RustRadio_{timestamp}.mp3")
            } else {
                format!("RustRadio_{timestamp}_{sequence}.mp3")
            };
            let path = folder.join(name);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => break (path, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => sequence += 1,
                Err(error) => return Err(error),
            }
        };
        let mut child = match mp3_encoder()
            .arg("pipe:1")
            .stdout(Stdio::from(file))
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&path);
                return Err(error);
            }
        };
        let mut stdin = child.stdin.take().expect("piped encoder input");
        let (sender, receiver) = mpsc::sync_channel::<Pcm>(100);
        let worker = thread::spawn(move || {
            let written = receiver
                .into_iter()
                .try_for_each(|block| stdin.write_all(&block));
            drop(stdin);
            let status = child.wait()?;
            if !status.success() {
                return Err(io::Error::other(format!(
                    "FFmpeg exited ({status}). Check libmp3lame and available disk space."
                )));
            }
            written
        });
        Ok((Self { path, worker }, sender))
    }

    pub fn is_finished(&self) -> bool {
        self.worker.is_finished()
    }

    pub fn finish(self) -> io::Result<()> {
        self.worker
            .join()
            .map_err(|_| io::Error::other("Recording worker stopped unexpectedly."))??;
        if fs::metadata(&self.path)?.len() == 0 {
            fs::remove_file(&self.path)?;
            return Err(io::Error::other("No audio was recorded."));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broadcast::BLOCK_SAMPLES;

    #[test]
    #[ignore = "requires FFmpeg with libmp3lame"]
    fn recording_flushes_mp3_when_the_feed_closes() -> io::Result<()> {
        let folder =
            std::env::temp_dir().join(format!("rust-radio-recording-{}", std::process::id()));
        fs::create_dir(&folder)?;
        let (recording, sender) = Recording::start(&folder)?;
        let path = recording.path.clone();
        for _ in 0..10 {
            sender
                .send([0; BLOCK_SAMPLES * 4])
                .map_err(io::Error::other)?;
        }
        drop(sender);
        recording.finish()?;
        let bytes = fs::read(&path)?;
        fs::remove_file(path)?;
        fs::remove_dir(folder)?;
        assert!(bytes.starts_with(&[0xff]) && bytes[1] & 0xe0 == 0xe0);
        Ok(())
    }
}
