# Changelog

## 0.2.2

- Added a waveform to the playback LCD. The played portion is highlighted, and clicking the waveform seeks within the track.
- Added LCDs for local system time and stream session duration.
- Placed the playback LCD beside the audio meters at wider window sizes and kept the playlist, jingles, and scheduler visible below.

## 0.2.1

- Added **Local volume** and **Local peak** to the main window. The peak meter reads the local signal after its volume control.
- Made local playback volume independent of the stream and MP3 recording. **Stream volume** controls the broadcast mix, including the microphone.
- Migrated the existing volume preference to **Local volume** and retained the saved stream level.

## 0.2.0

- First release with playlist import and export, shuffle, crossfades, jingles, and a scheduler.
- Added Icecast and SHOUTcast streaming, microphone mixing, a five-band equalizer, and MP3 recording.
- Published a portable Windows x64 package with FFmpeg.
