use std::collections::HashMap;
use std::env;
use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Duration;

const INDEX: &str = include_str!("index.html");
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct Room {
    text: String,
    listeners: Vec<mpsc::Sender<String>>,
}

type Rooms = Arc<Mutex<HashMap<String, Room>>>;

struct Request {
    method: String,
    path: String,
    body: String,
}

#[derive(Clone, Copy)]
enum Endpoint {
    Page,
    Raw,
    Events,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("clip: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None if io::stdin().is_terminal() => serve(),
        None => set(None),
        Some("serve") => serve(),
        Some("set") => set(args.get(1).map(String::as_str)),
        Some("get") => get(args.get(1).map(String::as_str)),
        Some("follow") => follow(args.get(1).map(String::as_str)),
        _ => {
            eprintln!("usage: clip [serve|set|get|follow] [/r/room]");
            std::process::exit(2);
        }
    }
}

fn serve() -> io::Result<()> {
    let address = env::var("CLIP_ADDR").unwrap_or_else(|_| "0.0.0.0:1984".into());
    let listener = TcpListener::bind(&address)?;
    let rooms: Rooms = Arc::new(Mutex::new(HashMap::new()));
    eprintln!("clip listening on http://127.0.0.1:1984");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let rooms = Arc::clone(&rooms);
                thread::spawn(move || {
                    if let Err(error) = handle(stream, rooms) {
                        eprintln!("clip: {error}");
                    }
                });
            }
            Err(error) => eprintln!("clip: {error}"),
        }
    }
    Ok(())
}

fn handle(mut stream: TcpStream, rooms: Rooms) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) if error.kind() == io::ErrorKind::InvalidData => {
            return response(&mut stream, "400 Bad Request", "text/plain", b"bad request");
        }
        Err(error) => return Err(error),
    };

    if request.method == "OPTIONS" {
        return response(&mut stream, "204 No Content", "text/plain", b"");
    }

    let Some((room, endpoint)) = route(&request.path) else {
        return response(&mut stream, "404 Not Found", "text/plain", b"not found");
    };

    match (request.method.as_str(), endpoint) {
        ("GET", Endpoint::Page) => response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            INDEX.as_bytes(),
        ),
        ("GET", Endpoint::Raw) => {
            let text = rooms
                .lock()
                .unwrap()
                .get(&room)
                .map(|room| room.text.clone())
                .unwrap_or_default();
            response(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                text.as_bytes(),
            )
        }
        ("GET", Endpoint::Events) => events(stream, rooms, room),
        ("PUT", Endpoint::Page) => {
            let mut rooms = rooms.lock().unwrap();
            let room = rooms.entry(room).or_default();
            room.text = request.body;
            room.listeners.retain(|listener| listener.send(room.text.clone()).is_ok());
            drop(rooms);
            response(&mut stream, "204 No Content", "text/plain", b"")
        }
        _ => response(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        ),
    }
}

fn route(request_path: &str) -> Option<(String, Endpoint)> {
    let path = request_path.split('?').next().unwrap_or("/");
    match path {
        "/" => Some(("/".into(), Endpoint::Page)),
        "/raw" => Some(("/".into(), Endpoint::Raw)),
        "/events" => Some(("/".into(), Endpoint::Events)),
        _ if path.starts_with("/r/") && path.len() > 3 => {
            if let Some(room) = path.strip_suffix("/raw") {
                Some((room.into(), Endpoint::Raw))
            } else if let Some(room) = path.strip_suffix("/events") {
                Some((room.into(), Endpoint::Events))
            } else {
                Some((path.trim_end_matches('/').into(), Endpoint::Page))
            }
        }
        _ => None,
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<Request> {
    let mut bytes = Vec::with_capacity(4096);
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "request ended"));
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > 64 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
        }
        if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break position + 4;
        }
    };

    let headers = std::str::from_utf8(&bytes[..header_end])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "headers are not utf-8"))?;
    let mut lines = headers.lines();
    let mut first = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?
        .split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first.next().unwrap_or("").to_string();
    if method.is_empty() || path.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad request line"));
    }

    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }

    while bytes.len() - header_end < content_length {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body ended"));
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    let body = String::from_utf8_lossy(&bytes[header_end..header_end + content_length]).into();
    Ok(Request { method, path, body })
}

fn response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, PUT, OPTIONS\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn events(mut stream: TcpStream, rooms: Rooms, name: String) -> io::Result<()> {
    let (sender, receiver) = mpsc::channel();
    let current = {
        let mut rooms = rooms.lock().unwrap();
        let room = rooms.entry(name).or_default();
        room.listeners.push(sender);
        room.text.clone()
    };

    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n",
    )?;
    write_event(&mut stream, &current)?;

    loop {
        match receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(text) => write_event(&mut stream, &text)?,
            Err(mpsc::RecvTimeoutError::Timeout) => stream.write_all(b": ping\n\n")?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn write_event(stream: &mut TcpStream, text: &str) -> io::Result<()> {
    if text.is_empty() {
        stream.write_all(b"data:\n\n")?;
    } else {
        for line in text.split('\n') {
            stream.write_all(b"data: ")?;
            stream.write_all(line.as_bytes())?;
            stream.write_all(b"\n")?;
        }
        stream.write_all(b"\n")?;
    }
    stream.flush()
}

fn set(room: Option<&str>) -> io::Result<()> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    let (_, body) = request("PUT", &room_path(room, ""), text.as_bytes())?;
    if !body.is_empty() {
        io::stderr().write_all(&body)?;
    }
    Ok(())
}

fn get(room: Option<&str>) -> io::Result<()> {
    let (_, body) = request("GET", &room_path(room, "/raw"), b"")?;
    io::stdout().write_all(&body)
}

fn follow(room: Option<&str>) -> io::Result<()> {
    let (mut stream, host) = connect()?;
    let path = room_path(room, "/events");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n"
    )?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
    }

    let mut event: Vec<String> = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if let Some(data) = line.strip_prefix("data:") {
            event.push(data.strip_prefix(' ').unwrap_or(data).to_string());
        } else if line.is_empty() && !event.is_empty() {
            println!("{}", event.join("\n"));
            event.clear();
        }
    }
}

fn room_path(room: Option<&str>, suffix: &str) -> String {
    let room = room
        .map(str::to_string)
        .or_else(|| env::var("CLIP_ROOM").ok())
        .unwrap_or_else(|| "/".into());
    let room = if room == "/" {
        String::new()
    } else if room.starts_with("/r/") {
        room.trim_end_matches('/').into()
    } else {
        format!("/r/{}", room.trim_matches('/'))
    };
    if room.is_empty() && suffix.is_empty() {
        "/".into()
    } else {
        format!("{room}{suffix}")
    }
}

fn connect() -> io::Result<(TcpStream, String)> {
    let url = env::var("CLIP_URL").unwrap_or_else(|_| "http://127.0.0.1:1984".into());
    let authority = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "CLIP_URL must use http://"))?
        .trim_end_matches('/');
    if authority.contains('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "CLIP_URL must not contain a path",
        ));
    }
    let address = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    Ok((TcpStream::connect(address)?, authority.into()))
}

fn request(method: &str, path: &str, body: &[u8]) -> io::Result<(u16, Vec<u8>)> {
    let (mut stream, host) = connect()?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    stream.shutdown(Shutdown::Write)?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let header_end = response
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad response"))?;
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad status"))?;
    let body = response[header_end + 4..].to_vec();
    if !(200..300).contains(&status) {
        return Err(io::Error::other(format!(
            "server returned {status}: {}",
            String::from_utf8_lossy(&body)
        )));
    }
    Ok((status, body))
}
