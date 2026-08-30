use std::{
    collections::HashMap,
    env,
    io::{self, BufRead, BufReader, IsTerminal, Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

const INDEX: &str = include_str!("index.html");
const MAX_BODY: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct Room {
    text: String,
    listeners: Vec<mpsc::Sender<String>>,
}

type Rooms = Arc<Mutex<HashMap<String, Room>>>;

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
    let mut args = env::args().skip(1);
    let command = args.next();
    let room = args.next();
    match command.as_deref() {
        None if io::stdin().is_terminal() => serve(),
        None => set(room.as_deref()),
        Some("serve") => serve(),
        Some("set") => set(room.as_deref()),
        Some("get") => get(room.as_deref()),
        Some("follow") => follow(room.as_deref()),
        Some("sync") => sync(room.as_deref()),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: clip [serve|set|get|follow|sync] [room]",
        )),
    }
}

fn serve() -> io::Result<()> {
    let address = env::var("CLIP_ADDR").unwrap_or_else(|_| "0.0.0.0:1984".into());
    let listener = TcpListener::bind(address)?;
    let rooms: Rooms = Arc::new(Mutex::new(HashMap::new()));
    eprintln!("clip listening on http://{}", listener.local_addr()?);

    for stream in listener.incoming() {
        let rooms = Arc::clone(&rooms);
        match stream {
            Ok(stream) => {
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
    let (method, path, body) = read_request(&mut stream)?;
    if method == "OPTIONS" {
        return respond(&mut stream, "204 No Content", "text/plain", b"");
    }
    let Some((name, endpoint)) = route(&path) else {
        return respond(&mut stream, "404 Not Found", "text/plain", b"not found");
    };

    match (method.as_str(), endpoint) {
        ("GET", Endpoint::Page) => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            INDEX.as_bytes(),
        ),
        ("GET", Endpoint::Raw) => {
            let text = rooms
                .lock()
                .unwrap()
                .get(&name)
                .map(|room| room.text.clone())
                .unwrap_or_default();
            respond(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                text.as_bytes(),
            )
        }
        ("GET", Endpoint::Events) => events(stream, rooms, name),
        ("PUT", Endpoint::Page) => {
            let mut rooms = rooms.lock().unwrap();
            let room = rooms.entry(name).or_default();
            room.text = body;
            room.listeners
                .retain(|tx| tx.send(room.text.clone()).is_ok());
            drop(rooms);
            respond(&mut stream, "204 No Content", "text/plain", b"")
        }
        _ => respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"method not allowed",
        ),
    }
}

fn route(request: &str) -> Option<(String, Endpoint)> {
    let path = request.split('?').next().unwrap_or("/");
    match path {
        "/" => Some(("/".into(), Endpoint::Page)),
        "/raw" => Some(("/".into(), Endpoint::Raw)),
        "/events" => Some(("/".into(), Endpoint::Events)),
        _ if path.starts_with("/r/") && path.len() > 3 => path
            .strip_suffix("/raw")
            .map(|room| (room.into(), Endpoint::Raw))
            .or_else(|| {
                path.strip_suffix("/events")
                    .map(|room| (room.into(), Endpoint::Events))
            })
            .or_else(|| Some((path.trim_end_matches('/').into(), Endpoint::Page))),
        _ => None,
    }
}

fn read_request(stream: &mut TcpStream) -> io::Result<(String, String, String)> {
    let mut bytes = Vec::new();
    let mut chunk = [0; 4096];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 || bytes.len() > 64 * 1024 {
            return invalid("bad request");
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
            break end + 4;
        }
    };

    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| bad("bad headers"))?;
    let mut lines = headers.lines();
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let method = request.next().unwrap_or("").to_string();
    let path = request.next().unwrap_or("").to_string();
    if method.is_empty() || path.is_empty() {
        return invalid("bad request line");
    }
    let length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0);
    if length > MAX_BODY {
        return invalid("body too large");
    }
    while bytes.len() - header_end < length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return invalid("short body");
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    let body = String::from_utf8_lossy(&bytes[header_end..header_end + length]).into();
    Ok((method, path, body))
}

fn invalid<T>(message: &str) -> io::Result<T> {
    Err(bad(message))
}

fn bad(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, PUT, OPTIONS\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn events(mut stream: TcpStream, rooms: Rooms, name: String) -> io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let current = {
        let mut rooms = rooms.lock().unwrap();
        let room = rooms.entry(name).or_default();
        room.listeners.push(tx);
        room.text.clone()
    };
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nAccess-Control-Allow-Origin: *\r\nConnection: keep-alive\r\nX-Accel-Buffering: no\r\n\r\n")?;
    write_event(&mut stream, &current)?;
    loop {
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(text) => write_event(&mut stream, &text)?,
            Err(mpsc::RecvTimeoutError::Timeout) => stream.write_all(b": ping\n\n")?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

fn write_event(stream: &mut TcpStream, text: &str) -> io::Result<()> {
    for line in text.split('\n') {
        writeln!(stream, "data: {line}")?;
    }
    stream.write_all(b"\n")?;
    stream.flush()
}

fn set(room: Option<&str>) -> io::Result<()> {
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    put(room, &text)
}

fn put(room: Option<&str>, text: &str) -> io::Result<()> {
    call("PUT", &room_path(room, ""), text.as_bytes()).map(|_| ())
}

fn get(room: Option<&str>) -> io::Result<()> {
    io::stdout().write_all(get_text(room)?.as_bytes())
}

fn get_text(room: Option<&str>) -> io::Result<String> {
    let body = call("GET", &room_path(room, "/raw"), b"")?;
    Ok(String::from_utf8_lossy(&body).into())
}

fn follow(room: Option<&str>) -> io::Result<()> {
    let (mut stream, host) = connect()?;
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream\r\nConnection: close\r\n\r\n",
        room_path(room, "/events")
    )?;
    let mut lines = BufReader::new(stream).lines();
    for line in lines.by_ref() {
        if line?.is_empty() {
            break;
        }
    }

    let mut event = Vec::new();
    for line in lines {
        let line = line?;
        if let Some(data) = line.strip_prefix("data: ") {
            event.push(data.to_string());
        } else if line.is_empty() && !event.is_empty() {
            println!("{}", event.join("\n"));
            event.clear();
        }
    }
    Ok(())
}

fn sync(room: Option<&str>) -> io::Result<()> {
    let mut local = clipboard_read()?;
    let mut remote = get_text(room)?;
    if remote.is_empty() {
        put(room, &local)?;
        remote = local.clone();
    } else if remote != local {
        clipboard_write(&remote)?;
        local = remote.clone();
    }
    eprintln!("clip syncing the macOS clipboard");

    loop {
        thread::sleep(Duration::from_millis(200));
        let next_local = clipboard_read()?;
        let next_remote = get_text(room)?;
        if next_local != local {
            put(room, &next_local)?;
            local = next_local.clone();
            remote = next_local;
        } else if next_remote != remote {
            clipboard_write(&next_remote)?;
            local = next_remote.clone();
            remote = next_remote;
        }
    }
}

fn clipboard_read() -> io::Result<String> {
    let output = Command::new("pbpaste").output()?;
    if !output.status.success() {
        return invalid("pbpaste failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).into())
}

fn clipboard_write(text: &str) -> io::Result<()> {
    let mut child = Command::new("pbcopy").stdin(Stdio::piped()).spawn()?;
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    if child.wait()?.success() {
        Ok(())
    } else {
        invalid("pbcopy failed")
    }
}

fn room_path(room: Option<&str>, suffix: &str) -> String {
    let room = room
        .map(str::to_string)
        .or_else(|| env::var("CLIP_ROOM").ok())
        .unwrap_or_default();
    let room = match room.as_str() {
        "" | "/" => String::new(),
        room if room.starts_with("/r/") => room.trim_end_matches('/').into(),
        room => format!("/r/{}", room.trim_matches('/')),
    };
    match (room.is_empty(), suffix.is_empty()) {
        (true, true) => "/".into(),
        _ => format!("{room}{suffix}"),
    }
}

fn connect() -> io::Result<(TcpStream, String)> {
    let url = env::var("CLIP_URL").unwrap_or_else(|_| "http://127.0.0.1:1984".into());
    let host = url
        .strip_prefix("http://")
        .map(|host| host.trim_end_matches('/'))
        .filter(|host| !host.contains('/'))
        .ok_or_else(|| bad("CLIP_URL must be http://host[:port]"))?;
    let address = if host.contains(':') {
        host.into()
    } else {
        format!("{host}:80")
    };
    Ok((TcpStream::connect(address)?, host.into()))
}

fn call(method: &str, path: &str, body: &[u8]) -> io::Result<Vec<u8>> {
    let (mut stream, host) = connect()?;
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())?;
    stream.write_all(body)?;
    stream.shutdown(Shutdown::Write)?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let end = response
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .ok_or_else(|| bad("bad response"))?;
    let status = String::from_utf8_lossy(&response[..end])
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| bad("bad status"))?;
    let body = response[end + 4..].to_vec();
    if (200..300).contains(&status) {
        Ok(body)
    } else {
        Err(io::Error::other(format!(
            "server returned {status}: {}",
            String::from_utf8_lossy(&body)
        )))
    }
}
