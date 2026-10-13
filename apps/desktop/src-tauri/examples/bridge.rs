//! Dev tool for end-to-end UI checks: serves the real Tauri commands (mock runtime) over local
//! HTTP so a headless browser can drive the built frontend. Not part of the product.
//!
//! `cargo run -p buturecad-desktop --example bridge -- <port>` then POST `/invoke/<cmd>` with the
//! JSON arguments. Binary replies (frames) are returned as `application/octet-stream`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use buturecad_desktop_lib::commands::AppState;
use buturecad_desktop_lib::workspace::Workspace;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::WebviewWindowBuilder;

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(7788);
    let recent = std::env::temp_dir().join("buturecad-e2e-recent.json");
    let app = buturecad_desktop_lib::with_commands(mock_builder())
        .manage(AppState(Mutex::new(Workspace::new(Some(recent)))))
        .build(mock_context(noop_assets()))
        .expect("app");
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("window");
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    eprintln!("bridge listening on {port}");
    for stream in listener.incoming().flatten() {
        let _ = handle(stream, &window);
    }
}

fn handle(
    mut stream: TcpStream,
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let mut parts = request_line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Methods: POST, OPTIONS\r\n";
    if method == "OPTIONS" {
        return stream.write_all(
            format!("HTTP/1.1 204 No Content\r\n{cors}Content-Length: 0\r\n\r\n").as_bytes(),
        );
    }
    let Some(cmd) = path.strip_prefix("/invoke/") else {
        return stream.write_all(
            format!("HTTP/1.1 404 Not Found\r\n{cors}Content-Length: 0\r\n\r\n").as_bytes(),
        );
    };
    let args: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
    let res = tauri::test::get_ipc_response(
        window,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .expect("url"),
            body: InvokeBody::Json(args),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    );
    let (status, ctype, payload): (&str, &str, Vec<u8>) = match res {
        Ok(InvokeResponseBody::Json(s)) => ("200 OK", "application/json", s.into_bytes()),
        Ok(InvokeResponseBody::Raw(b)) => ("200 OK", "application/octet-stream", b),
        Err(e) => (
            "500 Internal Server Error",
            "application/json",
            e.to_string().into_bytes(),
        ),
    };
    stream.write_all(
        format!(
            "HTTP/1.1 {status}\r\n{cors}Content-Type: {ctype}\r\nContent-Length: {}\r\n\r\n",
            payload.len()
        )
        .as_bytes(),
    )?;
    stream.write_all(&payload)
}
