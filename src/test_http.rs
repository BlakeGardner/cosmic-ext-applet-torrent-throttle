// SPDX-License-Identifier: GPL-3.0

//! Shared HTTP test helper for torrent client modules.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Minimal one-request-per-connection HTTP server. Serves the given
/// (status line, extra header lines, body) responses in order and returns
/// the raw captured requests. Every response carries `Connection: close`
/// so reqwest reconnects for each request.
pub async fn mock_server(
    responses: Vec<(&'static str, &'static str, &'static str)>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let mut captured = Vec::new();
        for (status, extra_headers, body) in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 65536];
            let mut read = 0;
            let request = loop {
                let n = stream.read(&mut buf[read..]).await.unwrap();
                read += n;
                let text = String::from_utf8_lossy(&buf[..read]).to_string();
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let content_length = text
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            if name.eq_ignore_ascii_case("content-length") {
                                value.trim().parse::<usize>().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if read >= header_end + 4 + content_length {
                        break text;
                    }
                }
                if n == 0 {
                    break text;
                }
            };
            captured.push(request);
            let response = format!(
                "HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{extra_headers}\r\n{body}",
                body.len(),
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
        captured
    });
    (format!("http://{}", addr), handle)
}
