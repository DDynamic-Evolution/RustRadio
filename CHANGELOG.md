# Changelog

## 0.2.3

- Start the stream timer after Icecast begins sending audio or SHOUTcast confirms source login. Retry lost connections every five seconds without stopping local playback or recording.
- Show pending items and the latest 100 log entries under **Scheduler → Queue and history**.
- Limit peaks in the broadcast and recording mix and show gain reduction beside the stream meter.
- Show the remaining track time and the next queued song in the playback LCD, including scheduled songs and shuffle order.

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
