use std::{
    collections::HashMap,
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use url::Url;

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(mime: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", mime.into())],
            body: body.into(),
        }
    }

    pub fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: vec![("Location", location.into())],
            body: Vec::new(),
        }
    }
}

pub struct Server {
    pub url: Url,
    worker: JoinHandle<io::Result<Vec<String>>>,
}

impl Server {
    pub fn new(routes: Vec<(&str, Reply)>, requests: usize) -> Self {
        let routes = routes
            .into_iter()
            .map(|(path, reply)| (path.to_owned(), reply))
            .collect::<HashMap<_, _>>();
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        listener.set_nonblocking(true).unwrap();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut paths = Vec::new();
            while paths.len() < requests {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("expected {requests} requests; got {paths:?}"),
                    ));
                }
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                let mut request = BufReader::new(&stream);
                let mut first = String::new();
                request.read_line(&mut first)?;
                let path = first
                    .split_ascii_whitespace()
                    .nth(1)
                    .ok_or_else(|| io::Error::other("missing request path"))?
                    .to_owned();
                loop {
                    let mut line = String::new();
                    if request.read_line(&mut line)? == 0 {
                        return Err(io::ErrorKind::UnexpectedEof.into());
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let reply = routes
                    .get(&path)
                    .ok_or_else(|| io::Error::other(format!("unexpected request: {path}")))?;
                write!(
                    stream,
                    "HTTP/1.1 {} Test\r\nConnection: close\r\n",
                    reply.status
                )?;
                if !reply
                    .headers
                    .iter()
                    .any(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                {
                    write!(stream, "Content-Length: {}\r\n", reply.body.len())?;
                }
                for (key, value) in &reply.headers {
                    write!(stream, "{key}: {value}\r\n")?;
                }
                stream.write_all(b"\r\n")?;
                stream.write_all(&reply.body)?;
                paths.push(path);
            }
            Ok(paths)
        });
        Self { url, worker }
    }

    pub fn finish(self) -> Vec<String> {
        self.worker.join().unwrap().unwrap()
    }
}
