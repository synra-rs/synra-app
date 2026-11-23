use std::io::{Read as _, Write as _};

/// A real one-request HTTP peer. Accept, read and write each have a deadline.
pub(super) fn respond(status: u16, body: &str) -> (String, std::thread::JoinHandle<String>) {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("free loopback port");
    listener.set_nonblocking(true).expect("bounded accept");
    let address = format!("http://{}", listener.local_addr().expect("address"));
    let body = body.to_owned();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::yield_now()
                }
                Err(error) => panic!("no request before deadline: {error}"),
            }
        };
        stream.set_nonblocking(false).expect("blocking connection");
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("read deadline");
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .expect("write deadline");
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            let count = stream.read(&mut bytes).expect("request");
            assert!(count > 0 && request.len() < 16_384, "bounded HTTP request");
            request.extend_from_slice(&bytes[..count]);
        }
        write!(
            stream,
            "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("response");
        String::from_utf8(request).expect("HTTP request text")
    });
    (address, server)
}
