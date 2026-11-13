use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
    time::Duration,
};

use eframe::egui;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned, pki_types::ServerName};
use serde::{Deserialize, Serialize};

pub const RATE: u32 = 44_100;
pub const CHANNELS: u16 = 2;
pub const BLOCK_SAMPLES: usize = 882 * CHANNELS as usize;
pub type Pcm = [u8; BLOCK_SAMPLES * 4];

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
enum Protocol {
    Icecast,
    Shoutcast,
}

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq)]
enum MetadataMode {
    #[default]
    Off,
    Track,
    Custom,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Settings {
    #[serde(default = "default_name")]
    pub name: String,
    protocol: Protocol,
    host: String,
    port: u16,
    mount: String,
    user: String,
    tls: bool,
    #[serde(default)]
    metadata: MetadataMode,
    #[serde(default)]
    custom_tag: String,
    #[serde(default)]
    metadata_port: u16,
    #[serde(skip)]
    metadata_password: String,
    #[serde(skip)]
    password: String,
}

fn default_name() -> String {
    "Stream 1".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: default_name(),
            protocol: Protocol::Icecast,
            host: "localhost".into(),
            port: 8000,
            mount: "/radio.mp3".into(),
            user: "source".into(),
            tls: false,
            metadata: MetadataMode::Off,
            custom_tag: String::new(),
            metadata_port: 0,
            metadata_password: String::new(),
            password: String::new(),
        }
    }
}

impl Settings {
    pub fn edit(&mut self, ui: &mut egui::Ui) {
        ui.label("Profile name");
        ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.protocol, Protocol::Icecast, "Icecast");
            ui.selectable_value(&mut self.protocol, Protocol::Shoutcast, "SHOUTcast (ICY)");
        });
        ui.label("Server");
        ui.add(egui::TextEdit::singleline(&mut self.host).desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            ui.label(if self.protocol == Protocol::Icecast {
                "Port"
            } else {
                "Source port"
            });
            ui.add(egui::DragValue::new(&mut self.port).range(1..=65535));
            ui.checkbox(&mut self.tls, "TLS");
        });
        if self.protocol == Protocol::Icecast {
            ui.label("Mount point");
            ui.add(egui::TextEdit::singleline(&mut self.mount).desired_width(f32::INFINITY));
            ui.label("Username");
            ui.add(egui::TextEdit::singleline(&mut self.user).desired_width(f32::INFINITY));
        }
        ui.label("Source password");
        ui.add(
            egui::TextEdit::singleline(&mut self.password)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        self.metadata_ui(ui);
    }

    fn metadata_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label("Stream metadata");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.metadata, MetadataMode::Off, "Off");
            ui.selectable_value(&mut self.metadata, MetadataMode::Track, "Track tags");
            ui.selectable_value(&mut self.metadata, MetadataMode::Custom, "Custom tag");
        });
        if self.metadata == MetadataMode::Custom {
            ui.add(
                egui::TextEdit::singleline(&mut self.custom_tag)
                    .hint_text("Custom stream title")
                    .desired_width(f32::INFINITY),
            );
        }
        if self.metadata != MetadataMode::Off && self.protocol == Protocol::Shoutcast {
            ui.horizontal(|ui| {
                ui.label("Metadata HTTP port");
                ui.add(egui::DragValue::new(&mut self.metadata_port).range(0..=65535));
                ui.label("0 = source port minus one");
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.metadata_password)
                    .password(true)
                    .hint_text("Admin password (optional)")
                    .desired_width(f32::INFINITY),
            );
        }
    }

    fn send_title(&self, title: &str) -> io::Result<()> {
        use crate::metadata::url_encode;
        use base64::{Engine, engine::general_purpose::STANDARD};

        let (port, path, auth) = match self.protocol {
            Protocol::Icecast => (
                self.port,
                format!(
                    "/admin/metadata?mode=updinfo&mount={}&song={}",
                    url_encode(&self.mount),
                    url_encode(title)
                ),
                format!(
                    "Authorization: Basic {}\r\n",
                    STANDARD.encode(format!("{}:{}", self.user, self.password))
                ),
            ),
            Protocol::Shoutcast => {
                let (source_password, sid) = self
                    .password
                    .rsplit_once(":#")
                    .unwrap_or((&self.password, "1"));
                let password = if self.metadata_password.is_empty() {
                    source_password
                } else {
                    &self.metadata_password
                };
                let port = if self.metadata_port == 0 {
                    self.port.saturating_sub(1)
                } else {
                    self.metadata_port
                };
                (
                    port,
                    format!(
                        "/admin.cgi?mode=updinfo&sid={}&pass={}&song={}",
                        url_encode(sid),
                        url_encode(password),
                        url_encode(title)
                    ),
                    String::new(),
                )
            }
        };
        let host = self.host.trim_matches(['[', ']']);
        let address = (host, port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::other("No metadata server address"))?;
        let socket = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        socket.set_write_timeout(Some(Duration::from_secs(3)))?;
        let mut connection: Box<dyn Connection> = if self.tls {
            let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            Box::new(tls_stream(socket, host.to_owned(), roots)?)
        } else {
            Box::new(socket)
        };
        let authority = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        write!(
            connection,
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\n{auth}User-Agent: RustRadio\r\nConnection: close\r\n\r\n"
        )?;
        connection.flush()?;
        let mut response = String::new();
        BufReader::new(connection)
            .take(1024)
            .read_line(&mut response)?;
        if !response
            .split_whitespace()
            .nth(1)
            .is_some_and(|status| status.starts_with('2') && status.len() == 3)
        {
            return Err(io::Error::other("The server rejected the metadata update"));
        }
        Ok(())
    }

    pub fn label(&self) -> &str {
        if self.name.trim().is_empty() {
            "Unnamed stream"
        } else {
            self.name.trim()
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.host.trim().is_empty()
            || self
                .host
                .chars()
                .any(|c| c.is_whitespace() || "/?#@".contains(c))
            || self.port == 0
            || self.password.is_empty()
            || self.password.contains(['\r', '\n'])
        {
            return Err("Check the server, port, and source password.".into());
        }
        if self.protocol == Protocol::Icecast
            && (!self.mount.starts_with('/')
                || self.mount == "/"
                || self
                    .mount
                    .chars()
                    .any(|c| c.is_whitespace() || "?#".contains(c))
                || self.user.is_empty()
                || self
                    .user
                    .bytes()
                    .any(|byte| !byte.is_ascii_alphanumeric() && !b"-._~".contains(&byte)))
        {
            return Err("Check the Icecast username and mount point.".into());
        }
        Ok(())
    }

    fn authority(&self) -> String {
        let host = self.host.trim_matches(['[', ']']);
        if host.contains(':') {
            format!("[{host}]:{}", self.port)
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

pub fn mp3_encoder() -> Command {
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "f32le",
            "-ar",
            "44100",
            "-ac",
            "2",
            "-i",
            "pipe:0",
            "-c:a",
            "libmp3lame",
            "-b:a",
            "128k",
            "-f",
            "mp3",
            "-write_xing",
            "0",
            "-id3v2_version",
            "0",
            "-flush_packets",
            "1",
        ])
        .stdin(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub struct Broadcast {
    child: Child,
    reports: Receiver<Result<(), String>>,
    ready: bool,
    tls: bool,
    pub name: String,
    metadata: MetadataUpdates,
}

impl Broadcast {
    pub fn start(settings: &Settings) -> Result<(Self, SyncSender<Pcm>), String> {
        settings.validate()?;
        let mut command = mp3_encoder();
        let authority = settings.authority();
        if settings.protocol == Protocol::Icecast {
            let user = &settings.user;
            let password = &settings.password;
            command.args([
                "-rw_timeout",
                "5000000",
                "-password",
                password,
                "-ice_name",
                "RustRadio",
            ]);
            if settings.tls {
                command.args(["-tls", "1", "-tls_verify", "1"]);
            }
            command.arg(format!("icecast://{user}@{authority}{}", settings.mount));
            command.stdout(Stdio::null());
        } else {
            command.arg("pipe:1").stdout(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start FFmpeg: {error}"))?;
        let (pcm_tx, pcm_rx) = mpsc::sync_channel::<Pcm>(50);
        let (reports_tx, reports) = mpsc::channel();
        let mut stdin = child.stdin.take().expect("piped encoder input");
        let input_reports = reports_tx.clone();
        thread::spawn(move || {
            for block in pcm_rx {
                if stdin.write_all(&block).is_err() {
                    let _ = input_reports.send(Err("FFmpeg closed its audio input.".into()));
                    break;
                }
            }
        });

        let ready = false;
        if let Some(mut stdout) = child.stdout.take() {
            let password = settings.password.clone();
            let tls_host = settings
                .tls
                .then(|| settings.host.trim_matches(['[', ']']).to_owned());
            thread::spawn(move || {
                let result =
                    shoutcast_connect(&authority, tls_host, &password).and_then(|mut socket| {
                        let _ = reports_tx.send(Ok(()));
                        io::copy(&mut stdout, &mut socket).map(|_| ())
                    });
                if let Err(error) = result {
                    let _ = reports_tx.send(Err(format!("SHOUTcast: {error}")));
                }
            });
        }

        Ok((
            Self {
                child,
                reports,
                ready,
                tls: settings.tls,
                name: settings.label().to_owned(),
                metadata: MetadataUpdates::new(settings.clone()),
            },
            pcm_tx,
        ))
    }

    pub fn poll(&mut self) -> Result<(), String> {
        for result in self.reports.try_iter() {
            result?;
            self.ready = true;
        }
        match self.child.try_wait() {
            Ok(None) => Ok(()),
            Ok(Some(status)) => Err(format!(
                "FFmpeg exited ({status}). Check the server settings and libmp3lame."
            )),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn update_title(&mut self, path: Option<&std::path::Path>) {
        self.metadata.update(path);
    }

    pub fn metadata_error(&self) -> Option<&str> {
        self.metadata.error.as_deref()
    }

    pub fn status(&self) -> &str {
        match (self.ready, self.tls) {
            (true, true) => "Connected to SHOUTcast over TLS",
            (true, false) => "Connected to SHOUTcast",
            (false, true) => "Encoder running · TLS enabled",
            (false, false) => "Encoder running",
        }
    }
}

struct MetadataUpdates {
    settings: Settings,
    last_path: Option<std::path::PathBuf>,
    sent: bool,
    request: Option<Receiver<io::Result<()>>>,
    retry_at: std::time::Instant,
    error: Option<String>,
}

impl MetadataUpdates {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            last_path: None,
            sent: false,
            request: None,
            retry_at: std::time::Instant::now() + Duration::from_secs(1),
            error: None,
        }
    }

    fn update(&mut self, path: Option<&std::path::Path>) {
        let path = match self.settings.metadata {
            MetadataMode::Off => return,
            MetadataMode::Custom => None,
            MetadataMode::Track => path,
        };
        if let Some(request) = &self.request {
            match request.try_recv() {
                Ok(Ok(())) => {
                    self.sent = true;
                    self.error = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(format!("Metadata: {error}"));
                    self.retry_at = std::time::Instant::now() + Duration::from_secs(5);
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.retry_at = std::time::Instant::now() + Duration::from_secs(5);
                }
            }
            self.request = None;
        }

        if self.last_path.as_deref() != path {
            self.last_path = path.map(ToOwned::to_owned);
            self.sent = false;
        }
        if self.sent || std::time::Instant::now() < self.retry_at {
            return;
        }
        let settings = self.settings.clone();
        let path = self.last_path.clone();
        let (sender, receiver) = mpsc::channel();
        self.request = Some(receiver);
        thread::spawn(move || {
            let title = match settings.metadata {
                MetadataMode::Custom => settings.custom_tag.clone(),
                _ => path
                    .as_deref()
                    .map(crate::metadata::track_title)
                    .unwrap_or_default(),
            };
            let _ = sender.send(settings.send_title(&title));
        });
    }
}

impl Drop for Broadcast {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

trait Connection: Read + Write {}
impl<T: Read + Write> Connection for T {}

fn shoutcast_connect(
    authority: &str,
    tls_host: Option<String>,
    password: &str,
) -> io::Result<Box<dyn Connection>> {
    let mut last_error = io::Error::new(io::ErrorKind::AddrNotAvailable, "No server address found");
    for address in authority.to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, Duration::from_secs(5)) {
            Ok(socket) => {
                let timeout = Some(Duration::from_secs(5));
                socket.set_read_timeout(timeout)?;
                socket.set_write_timeout(timeout)?;
                let mut connection: Box<dyn Connection> = match tls_host {
                    Some(host) => {
                        let roots = RootCertStore::from_iter(
                            webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
                        );
                        Box::new(tls_stream(socket, host, roots)?)
                    }
                    None => Box::new(socket),
                };
                shoutcast_login(&mut connection, password)?;
                return Ok(connection);
            }
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn tls_stream(
    socket: TcpStream,
    host: String,
    roots: RootCertStore,
) -> io::Result<StreamOwned<ClientConnection, TcpStream>> {
    let name = ServerName::try_from(host).map_err(io::Error::other)?;
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let client =
        ClientConnection::new(std::sync::Arc::new(config), name).map_err(io::Error::other)?;
    Ok(StreamOwned::new(client, socket))
}

fn shoutcast_login(mut socket: impl Read + Write, password: &str) -> io::Result<()> {
    write!(socket, "{password}\r\n")?;
    let mut response = String::new();
    BufRead::read_line(&mut BufReader::new(&mut socket).take(1024), &mut response)?;
    if response.trim() != "OK2" {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Source login rejected",
        ));
    }
    socket.write_all(
        b"icy-name:RustRadio\r\nicy-br:128\r\nicy-pub:0\r\ncontent-type:audio/mpeg\r\n\r\n",
    )?;
    socket.flush()
}

pub fn send_pcm(sender: &mut Option<SyncSender<Pcm>>, block: Pcm) {
    if let Some(channel) = sender
        && let Err(TrySendError::Disconnected(_)) = channel.try_send(block)
    {
        *sender = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    #[ignore = "requires FFmpeg with libmp3lame and loopback TCP"]
    fn streams_mp3_to_icecast() {
        verify_stream(Protocol::Icecast);
    }

    #[test]
    #[ignore = "requires FFmpeg with libmp3lame and loopback TCP"]
    fn streams_mp3_to_shoutcast() {
        verify_stream(Protocol::Shoutcast);
    }

    #[test]
    fn shoutcast_login_uses_verified_tls() {
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.der().clone()], key.into())
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(cert.der().clone()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let connection = rustls::ServerConnection::new(std::sync::Arc::new(config)).unwrap();
            let mut stream = StreamOwned::new(connection, socket);
            let mut login = String::new();
            BufReader::new(&mut stream).read_line(&mut login).unwrap();
            assert_eq!(login, "tls-test\r\n");
            stream.write_all(b"OK2\r\n").unwrap();
            stream.flush().unwrap();
            let mut header = String::new();
            BufReader::new(&mut stream).read_line(&mut header).unwrap();
            assert_eq!(header, "icy-name:RustRadio\r\n");
        });
        let socket = TcpStream::connect(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut stream = tls_stream(socket, "localhost".into(), roots).unwrap();
        shoutcast_login(&mut stream, "tls-test").unwrap();
        server.join().unwrap();
    }

    #[test]
    fn metadata_requests_encode_titles_and_select_the_correct_stream() {
        for protocol in [Protocol::Icecast, Protocol::Shoutcast] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let settings = Settings {
                protocol,
                host: "127.0.0.1".into(),
                port,
                metadata_port: port,
                password: if protocol == Protocol::Icecast {
                    "demo"
                } else {
                    "demo:#7"
                }
                .into(),
                ..Settings::default()
            };
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = String::new();
                let mut reader = BufReader::new(&mut socket);
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                if protocol == Protocol::Icecast {
                    assert!(
                        request.starts_with(
                            "GET /admin/metadata?mode=updinfo&mount=%2Fradio.mp3&song="
                        )
                    );
                    assert!(request.contains("Authorization: Basic c291cmNlOmRlbW8="));
                } else {
                    assert!(
                        request.starts_with("GET /admin.cgi?mode=updinfo&sid=7&pass=demo&song=")
                    );
                }
                assert!(request.contains("Gr%C3%BCne%20Welle%20%26%20Co HTTP/1.1"));
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
            });
            settings.send_title("Grüne Welle & Co").unwrap();
            server.join().unwrap();
        }
    }

    fn verify_stream(protocol: Protocol) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let settings = Settings {
            protocol,
            host: "127.0.0.1".into(),
            port: listener.local_addr().unwrap().port(),
            password: "test:word".into(),
            ..Settings::default()
        };
        let (received_tx, received_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            if protocol == Protocol::Shoutcast {
                let mut login = String::new();
                reader.read_line(&mut login).unwrap();
                assert_eq!(login.trim(), "test:word");
                socket.write_all(b"OK2\r\nicy-caps:11\r\n\r\n").unwrap();
            }
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                if line == "\r\n" {
                    break;
                }
                headers.push_str(&line);
            }
            if protocol == Protocol::Icecast {
                assert!(headers.starts_with("PUT /radio.mp3 "));
                assert!(headers.contains("Authorization: Basic c291cmNlOnRlc3Q6d29yZA=="));
                socket.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
                socket.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap();
            } else {
                assert!(headers.contains("icy-br:128"));
            }
            let mut audio = [0; 1024];
            reader.read_exact(&mut audio).unwrap();
            assert_eq!(audio[0], 0xff);
            assert_eq!(audio[1] & 0xe0, 0xe0);
            received_tx.send(()).unwrap();
        });
        let (mut broadcast, input) = Broadcast::start(&settings).unwrap();
        let mut received = false;
        for _ in 0..250 {
            if received_rx.try_recv().is_ok() {
                received = true;
                break;
            }
            broadcast.poll().unwrap();
            let _ = input.try_send([0; BLOCK_SAMPLES * 4]);
            thread::sleep(Duration::from_millis(20));
        }
        assert!(received, "no MP3 data received within 5 seconds");
        drop(broadcast);
        server.join().unwrap();
    }
}
