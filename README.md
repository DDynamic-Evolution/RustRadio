# RustRadio

RustRadio is a desktop radio automation and streaming app written in Rust for Windows and Linux, inspired by RadioBOSS.

## Download for Windows x64

1. Download the [portable ZIP](https://github.com/DDynamic-Evolution/RustRadio/releases/download/v0.2.2/RustRadio-v0.2.2-windows-x64-portable.zip) from [release v0.2.2](https://github.com/DDynamic-Evolution/RustRadio/releases/tag/v0.2.2).
2. Extract the entire archive.
3. Double-click **Start RustRadio.cmd**. Keep the `ffmpeg` folder next to the launcher.

The portable package includes FFmpeg for streaming and MP3 recording. You can run it without Rust, Cargo, or a separate FFmpeg installation. The release also offers a standalone EXE that looks for FFmpeg in `PATH`.

See [CHANGELOG.md](CHANGELOG.md) for release changes.

## Build from source

Install Rust 1.88 or later. RustRadio runs in a desktop session with audio output. On Windows, use the MSVC Rust toolchain and Visual Studio Build Tools.

On Ubuntu or Debian, install the build dependencies:

```sh
sudo apt install build-essential pkg-config libasound2-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev
```

The Linux file picker uses the desktop environment's portal. You can also drag audio files into the app.

```sh
cargo run --locked
cargo build --release --locked
```

The release executable is `target/release/rust-radio.exe` on Windows and `target/release/rust-radio` on Linux. To stream or record with a source build, install FFmpeg with `libmp3lame` and make it available in `PATH`. On Ubuntu and Debian, install the `ffmpeg` package.

## Playback and scheduling

The playback LCD shows the current track and its waveform. Click the waveform to seek. Separate LCDs show local system time and the current stream session duration.

- **Playlist** lets you add, remove, and reorder tracks. Double-click a track to play it. **Save playlist** exports M3U8. **Load playlist** opens M3U8 and UTF-8 M3U files, stops playback, and replaces the current list. Relative audio paths are resolved from the playlist's folder.
- **Shuffle** plays tracks in random order, avoiding immediate repeats between passes when multiple tracks are available. With Shuffle off, playback follows playlist order and stops at the end.
- **Crossfade** under **Settings → Automation** blends into the next track over up to 15 seconds when the track duration is known. Short tracks fade over at most their second half.
- **Jingles → Add clips** stores station IDs, jingles, and sound effects alongside the playlist. Click a clip to play it over the program in both the local output and stream. A new clip replaces the previous one. **Stop clip** stops it independently of the playlist.
- **Scheduler** accepts audio files, playlist tracks, and jingles. Set a local time and either weekdays or a **One-time date**. **Clip** plays over ducked music. **Song · on time** interrupts the playlist. **Song · after current track** waits for the track to finish. RustRadio must be open during the scheduled minute.

## Microphone and audio

Use **Settings → Audio** to choose the microphone and playback device. Select **System default** to use the operating system's default device. **Refresh devices** updates the list after plugging in a device. RustRadio remembers both selections.

**Microphone** adds your voice to the stream and reduces music to 25% of its current level. Turning the microphone off restores the music level. Local monitoring plays the ducked music. **Mic volume** sets your voice level and **Mic peak** measures it in dBFS before mixing.

**Settings → Equalizer** applies five bands from 80 Hz to 12 kHz to the music in both the stream and local output. Each band has a ±12 dB range. EQ settings are saved automatically.

**Local volume** adjusts playback on the selected output device. **Local peak** shows the signal after that control. **Stream volume** controls the final mix for streaming and recording, including the microphone. **Stream peak** measures the signal before encoding and works before connecting to a server.

## Streaming

Under **Settings → Streams**, use **New profile** to save named streaming destinations. The profiles and selected profile are restored on restart. Source passwords are kept for the current session.

- **Icecast** requires a server, port, mount point, username, and source password. Enable **TLS** if the server supports it. RustRadio passes the source password to FFmpeg as an argument.
- **SHOUTcast (ICY)** requires a source port and source password. The source port is often the listener port plus one. SHOUTcast v2 must accept legacy sources. If your provider specifies a stream ID, use its required password format, commonly `password:#streamid`. TLS requires a TLS-enabled source port or proxy. RustRadio validates the certificate and hostname against public root certificates.

Click **Connect** in the main window to start the selected profile. RustRadio sends the stereo program mix as 128 kbit/s MP3. The connection stays open during silence.

Under **Stream metadata**, choose **Off**, **Track tags** (artist and title, with the filename as a fallback), or **Custom tag** (fixed text). Reconnect to apply changes to an active profile. For SHOUTcast, set the HTTP admin port if it differs from the source port minus one. Enter an admin password if the source password cannot update titles.

## Recording

Choose a folder under **Settings → Recording**. **Record MP3** records the final stream mix at 128 kbit/s, including when no server is connected. **Stop recording** finalizes the timestamped file.

## Tests

Run the optional FFmpeg protocol and recording tests with FFmpeg and `libmp3lame` available in `PATH`:

```sh
cargo test --locked -- --include-ignored
```
