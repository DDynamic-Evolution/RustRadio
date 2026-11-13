# RustRadio

Eine Radioautomation in Rust für Windows und Linux, inspiriert von RadioBOSS.

## Starten

Rust 1.88 oder neuer und eine Desktop-Sitzung mit Audioausgabe werden benötigt. Unter Windows wird die Rust-Toolchain für MSVC mit den Visual Studio Build Tools benötigt.

Unter Ubuntu/Debian:

```bash
sudo apt install build-essential pkg-config libasound2-dev libxkbcommon-dev libwayland-dev libgl1-mesa-dev
```

Der Linux-Dateidialog verwendet das Desktop-Portal der jeweiligen Desktop-Umgebung. Dateien lassen sich auch per Drag-and-drop hinzufügen.

Im Projektordner starten:

```sh
cargo run --locked
```

## Bedienung

- Audiodateien hinzufügen, auswählen, verschieben und entfernen.
- Die Wiedergabe per Doppelklick oder über die Schaltflächen steuern.
- Am Ende eines Titels startet der nächste lesbare Eintrag automatisch. Am Ende der Liste stoppt die Wiedergabe.
- Playlist und Lautstärke werden für den nächsten Start gespeichert.


## Automation

- **Sendeplanung** startet Audiodateien an ausgewählten Wochentagen zur lokalen Uhrzeit. Termine unterbrechen das laufende Programm. Gleichzeitig fällige Termine laufen nacheinander. Danach geht die Playlist mit dem nächsten Titel weiter. Die App muss zur geplanten Minute geöffnet sein.
- **Shuffle** im Hauptfenster mischt die Playlist pro Durchlauf und wiederholt bei mehreren Titeln den letzten Titel nicht direkt. Ohne Rotation läuft die Playlist in Listenreihenfolge.
- **Crossfade** überblendet automatisch auf den nächsten Titel, wenn die Dauer der laufenden Datei bekannt ist. Der Regler reicht von 0 bis 15 Sekunden. Kurze Titel werden maximal über ihre zweite Hälfte ausgeblendet.

## Settings → Streams

Über **Neues Profil** lassen sich mehrere Streaming-Ziele mit eigenen Namen speichern. Die Auswahl und die Profile bleiben beim nächsten Start erhalten. Das Fenster ist frei skalierbar und bei geringer Höhe scrollbar.

FFmpeg mit dem Encoder `libmp3lame` muss im `PATH` liegen. Unter Ubuntu/Debian ist FFmpeg als Paket `ffmpeg` verfügbar. Die App sendet den gleichen Stereo-Mix wie die lokale Audioausgabe als MP3 mit 128 kbit/s. Bei Pause und Stopp bleibt die Verbindung mit Stille bestehen.

- **Icecast** verwendet Server, Port, Mountpoint, Benutzer und Quellpasswort. TLS kann im Dialog aktiviert werden. FFmpeg übernimmt die Icecast-Verbindung.
- **SHOUTcast (ICY)** verwendet den Source-Port und das Quellpasswort. Für TLS muss der Source-Port oder ein vorgeschalteter Proxy TLS anbieten. RustRadio prüft Zertifikat und Hostnamen gegen öffentliche Stammzertifikate. Bei vielen Servern ist der Source-Port der Hörer-Port plus eins. Für SHOUTcast v2 muss der Server Legacy-Quellen akzeptieren. Bei einer Stream-ID im Legacy-Modus das vom Anbieter vorgegebene Passwortformat verwenden (häufig `passwort:#streamid`).

Die Quellpasswörter bleiben in der laufenden App und werden nicht gespeichert. Bei Icecast wird das Passwort als Argument an FFmpeg übergeben. Zum Starten des Streams im Dialog **Verbinden** wählen.

Die Protokolltests können mit installiertem FFmpeg lokal gestartet werden:

```sh
cargo test --locked -- --include-ignored
```


## Microphone and equalizer

**Microphone** uses the operating system's default input and adds it to the stream. Music fades to 25 percent while the mic is on and returns to the Volume setting when it is off. Local monitoring plays the ducked music without microphone playback.

**Settings → Equalizer** applies five bands from 80 Hz to 12 kHz to the music in both the stream and local output. Each band has a ±12 dB range, and settings are saved automatically.

Use **Settings → Audio** to choose a microphone input and playback output or select **System default** for either. The app remembers both choices. Use **Refresh devices** after plugging in a device.


The main window's **Stream volume** controls the final mix, including the microphone. **Stream peak** displays the pre-encoder signal in dBFS and works before connecting to a server.


Use **Save playlist** to export an M3U8 file. **Load playlist** opens M3U8 or UTF-8 M3U files, stops playback and replaces the current list. Relative audio paths are resolved from the playlist file's folder.

Use **Jingles → Add clips** to keep station IDs, jingles, and sound effects beside the playlist. Click a clip to play it over the current program in both the local output and stream. A new clip replaces the previous clip. **Stop clip** stops the clip independently of the playlist.

Use **Connect** in the main window to start the selected profile. Under **Settings → Streams → Stream metadata**, choose **Off**, **Track tags** (artist and title, with the filename as fallback), or **Custom tag** (fixed text). Reconnect after changing the active profile's metadata settings. For SHOUTcast, set the HTTP admin port if it differs from the source port minus one. Enter an admin password if the source password cannot update titles.

Choose the destination under **Settings → Recording**. **Record MP3** records the final stream mix at 128 kbit/s, even without a server connection. **Stop recording** finalizes the timestamped file. Recording uses the same FFmpeg installation as streaming.

Use the **Scheduler** column to add an audio file or choose an entry from **From playlist** or **From jingles**. Set the local time and either weekdays or a **One-time date**. **Clip** plays over ducked music, **Song · on time** interrupts the playlist, and **Song · after current track** waits for the track to finish.
