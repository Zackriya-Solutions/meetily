//! Seasonal sidebar icon (official builds only).
//! When MEETILY_DOODLE_URL is set at compile time, the app requests a seasonal icon over HTTPS.
//! Forks and local builds without the variable make no request.

use chrono::NaiveDate;
use reqwest::{StatusCode, Url};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::Manager;
use tokio::fs;

const DOODLE_URL: Option<&str> = option_env!("MEETILY_DOODLE_URL");
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const MAX_BYTES: usize = 256 * 1024;
const MAX_PIXELS: u32 = 512;
const MAX_REDIRECTS: usize = 2;
const TIMEOUT: Duration = Duration::from_secs(10);
const ICON_FILE: &str = "doodle.png";
const TMP_FILE: &str = "doodle.png.tmp";
const LAST_CHECK_FILE: &str = "last-check";

fn configured_url(raw: Option<&'static str>) -> Option<&'static str> {
    raw.filter(|url| url.starts_with("https://"))
}

fn redirect_allowed(origin: &Url, next: &Url, previous: usize) -> bool {
    previous <= MAX_REDIRECTS
        && next.scheme() == origin.scheme()
        && next.host_str() == origin.host_str()
        && next.port_or_known_default() == origin.port_or_known_default()
        && next.path().starts_with("/doodle/")
}

fn build_client(origin: Url, https_only: bool) -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(https_only)
        .timeout(TIMEOUT)
        .referer(false)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if redirect_allowed(&origin, attempt.url(), attempt.previous().len()) {
                attempt.follow()
            } else {
                perf_debug!("doodle: redirect rejected by origin/path/hop policy");
                attempt.stop()
            }
        }))
        .build()
}

fn is_valid_png(bytes: &[u8]) -> bool {
    if bytes.len() < 24 || bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return false;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (1..=MAX_PIXELS).contains(&width) && (1..=MAX_PIXELS).contains(&height)
}

async fn fetch(client: &reqwest::Client, url: &str) -> reqwest::Result<Option<Vec<u8>>> {
    let mut response = client.get(url).send().await?;
    if response.status() != StatusCode::OK {
        perf_debug!("doodle: no icon returned (HTTP {})", response.status());
        return Ok(None);
    }
    let mut buf = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if buf.len() + chunk.len() > MAX_BYTES {
            perf_debug!("doodle: body exceeds {} bytes (received at least {})", MAX_BYTES, buf.len() + chunk.len());
            return Ok(None);
        }
        buf.extend_from_slice(&chunk);
    }
    let valid = is_valid_png(&buf);
    if !valid {
        perf_debug!("doodle: invalid PNG header or dimensions ({} bytes, maximum {} px)", buf.len(), MAX_PIXELS);
    }
    Ok(valid.then_some(buf))
}

fn sync_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn sync(
    client: &reqwest::Client,
    url: &str,
    dir: &Path,
    today: NaiveDate,
) -> Result<Option<PathBuf>, String> {
    let _guard = sync_lock().lock().await;
    let icon = dir.join(ICON_FILE);
    let last_check = dir.join(LAST_CHECK_FILE);
    let today = today.to_string();
    if let Ok(saved_date) = fs::read_to_string(&last_check).await {
        if saved_date.trim() == today {
            return Ok(fs::try_exists(&icon)
                .await
                .map_err(|e| e.to_string())?
                .then_some(icon));
        }
    }
    fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
    let temporary = dir.join(TMP_FILE);
    let outcome = fetch(client, url).await;
    let result: std::io::Result<Option<PathBuf>> = async {
        match outcome {
            Ok(Some(bytes)) => {
                fs::write(&temporary, bytes).await?;
                fs::rename(&temporary, &icon).await?;
                fs::write(&last_check, &today).await?;
                Ok(Some(icon))
            }
            response => {
                if let Err(e) = fs::remove_file(&icon).await {
                    if e.kind() != ErrorKind::NotFound {
                        return Err(e);
                    }
                }
                if let Err(e) = response {
                    log::warn!("doodle: request failed: {e}");
                } else {
                    fs::write(&last_check, &today).await?;
                }
                Ok(None)
            }
        }
    }
    .await;
    if matches!(&result, Ok(Some(_))) {
        log::info!("doodle: saved icon");
    } else {
        log::info!("doodle: no icon");
    }
    if result.is_err() {
        let _ = fs::remove_file(&temporary).await;
    }
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn doodle_sync(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(url) = configured_url(DOODLE_URL) else {
        return Ok(None);
    };
    let origin = Url::parse(url).map_err(|e| e.to_string())?;
    let client = build_client(origin, true).map_err(|e| e.to_string())?;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("doodle");
    Ok(sync(&client, url, &dir, chrono::Utc::now().date_naive())
        .await?
        .map(|path| path.to_string_lossy().into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&w.to_be_bytes());
        bytes.extend_from_slice(&h.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0; 4]);
        bytes
    }

    fn response(status: &str, body: &[u8]) -> Vec<u8> {
        let mut bytes = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }

    fn redirect(location: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
        .into_bytes()
    }

    async fn read_request(socket: &mut TcpStream) -> String {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let count = socket.read(&mut buffer).await.unwrap();
            assert_ne!(count, 0, "connection closed before request headers");
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                return String::from_utf8(request).unwrap();
            }
        }
    }

    async fn server(
        handler: impl Fn(&str) -> Vec<u8> + Send + Sync + 'static,
    ) -> (Url, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = Url::parse(&format!("http://{addr}/doodle?app=oss")).unwrap();
        let counter = Arc::new(AtomicUsize::new(0));
        let requests = counter.clone();
        let handler = Arc::new(handler);
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let requests = requests.clone();
                let handler = handler.clone();
                tokio::spawn(async move {
                    let request = read_request(&mut socket).await;
                    requests.fetch_add(1, Ordering::SeqCst);
                    let headers = request.to_ascii_lowercase();
                    for header in ["user-agent:", "cookie:", "referer:", "authorization:"] {
                        assert!(!headers.contains(header), "unexpected {header} header");
                    }
                    let path = request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                    // Oversized responses may be dropped before the server finishes writing.
                    let _ = socket.write_all(&handler(path)).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (origin, counter)
    }

    fn client(origin: &Url) -> reqwest::Client {
        build_client(origin.clone(), false).unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 9).unwrap()
    }

    async fn assert_date_saved(dir: &Path) {
        assert_eq!(
            tokio::fs::read_to_string(dir.join(LAST_CHECK_FILE)).await.unwrap(),
            today().to_string()
        );
    }

    #[test]
    fn configuration_requires_explicit_https_url() {
        assert_eq!(configured_url(None), None);
        assert_eq!(configured_url(Some("")), None);
        assert_eq!(configured_url(Some("http://example.invalid/doodle")), None);
        assert_eq!(
            configured_url(Some("https://example.invalid/doodle")),
            Some("https://example.invalid/doodle")
        );
    }

    #[test]
    fn png_header_requires_signature_ihdr_and_bounded_nonzero_dimensions() {
        assert!(is_valid_png(&png(1, 1)));
        assert!(is_valid_png(&png(512, 512)));
        for (width, height) in [(0, 1), (1, 0), (513, 10), (10, 513)] {
            assert!(!is_valid_png(&png(width, height)));
        }
        assert!(!is_valid_png(&png(1, 1)[..23]));
        let mut bytes = png(1, 1);
        bytes[0] = 0;
        assert!(!is_valid_png(&bytes));
        let mut bytes = png(1, 1);
        bytes[12..16].copy_from_slice(b"IDAT");
        assert!(!is_valid_png(&bytes));
    }

    #[test]
    fn redirect_policy_restricts_origin_path_and_hop_count() {
        let origin = Url::parse("https://example.invalid/doodle").unwrap();
        let next = origin.join("/doodle/a.png").unwrap();
        assert!(redirect_allowed(&origin, &next, 1));
        assert!(redirect_allowed(&origin, &next, 2));
        assert!(!redirect_allowed(&origin, &next, 3));
        let downgrade = Url::parse("http://example.invalid/doodle/a.png").unwrap();
        assert!(!redirect_allowed(&origin, &downgrade, 1));
        let mut other_port = next.clone();
        other_port.set_port(Some(8443)).unwrap();
        assert!(!redirect_allowed(&origin, &other_port, 1));
        assert!(!redirect_allowed(&origin, &origin.join("/other/a.png").unwrap(), 1));
    }

    #[tokio::test]
    async fn same_day_cache_does_not_request_with_or_without_icon() {
        let dir = tempfile::tempdir().unwrap();
        let (origin, count) = server(|_| response("200 OK", &png(1, 1))).await;
        let client = client(&origin);
        let icon = dir.path().join(ICON_FILE);
        tokio::fs::write(&icon, png(1, 1)).await.unwrap();
        tokio::fs::write(dir.path().join(LAST_CHECK_FILE), format!("{}\n", today()))
            .await
            .unwrap();
        assert_eq!(sync(&client, origin.as_str(), dir.path(), today()).await.unwrap(), Some(icon.clone()));
        tokio::fs::remove_file(&icon).await.unwrap();
        assert_eq!(sync(&client, origin.as_str(), dir.path(), today()).await.unwrap(), None);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn allowed_redirect_saves_png_and_date() {
        let dir = tempfile::tempdir().unwrap();
        let (origin, count) = server(|path| match path {
            "/doodle?app=oss" => redirect("/doodle/a.png?app=oss"),
            "/doodle/a.png?app=oss" => response("200 OK", &png(512, 512)),
            _ => panic!("unexpected path: {path}"),
        }).await;
        let icon = dir.path().join(ICON_FILE);
        assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), Some(icon.clone()));
        assert_eq!(tokio::fs::read(icon).await.unwrap(), png(512, 512));
        assert_date_saved(dir.path()).await;
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn invalid_png_and_excessive_dimensions_save_date_without_icon() {
        for body in [b"<svg/>".to_vec(), png(513, 10)] {
            let dir = tempfile::tempdir().unwrap();
            let (origin, count) = server(move |_| response("200 OK", &body)).await;
            assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), None);
            assert!(!tokio::fs::try_exists(dir.path().join(ICON_FILE)).await.unwrap());
            assert_date_saved(dir.path()).await;
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn oversized_body_without_content_length_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let (origin, _) = server(|_| {
            let mut bytes = b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_vec();
            let mut body = png(1, 1);
            body.resize(300 * 1024, 0);
            bytes.extend_from_slice(&body);
            bytes
        }).await;
        assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), None);
        assert!(!tokio::fs::try_exists(dir.path().join(ICON_FILE)).await.unwrap());
        assert_date_saved(dir.path()).await;
    }

    #[tokio::test]
    async fn oversized_stream_is_rejected_without_waiting_for_body_end() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = Url::parse(&format!("http://{addr}/doodle?app=oss")).unwrap();
        let (_hold_open, released) = tokio::sync::oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000000\r\nConnection: close\r\n\r\n").await.unwrap();
            let mut body = png(1, 1);
            body.resize(300 * 1024, 0);
            let _ = socket.write_all(&body).await;
            let _ = released.await;
            let _ = socket.shutdown().await;
        });
        let client = client(&origin);
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            sync(&client, origin.as_str(), dir.path(), today()),
        ).await;
        task.abort();
        assert_eq!(result.expect("must stop reading at the byte limit").unwrap(), None);
        assert!(!tokio::fs::try_exists(dir.path().join(ICON_FILE)).await.unwrap());
        assert_date_saved(dir.path()).await;
    }

    #[tokio::test]
    async fn non_ok_responses_remove_stale_icon_and_save_date() {
        for status in ["204 No Content", "500 Internal Server Error"] {
            let dir = tempfile::tempdir().unwrap();
            let icon = dir.path().join(ICON_FILE);
            tokio::fs::write(&icon, png(1, 1)).await.unwrap();
            tokio::fs::write(dir.path().join(LAST_CHECK_FILE), "2026-11-08").await.unwrap();
            let (origin, count) = server(move |_| response(status, b"")).await;
            assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), None);
            assert!(!tokio::fs::try_exists(icon).await.unwrap());
            assert_date_saved(dir.path()).await;
            assert_eq!(count.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn connection_failure_removes_stale_icon_without_saving_date() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join(ICON_FILE);
        tokio::fs::write(&icon, png(1, 1)).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let origin = Url::parse(&format!("http://{addr}/doodle?app=oss")).unwrap();
        drop(listener);
        assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), None);
        assert!(!tokio::fs::try_exists(icon).await.unwrap());
        assert!(!tokio::fs::try_exists(dir.path().join(LAST_CHECK_FILE)).await.unwrap());
    }

    #[tokio::test]
    async fn redirects_to_different_host_or_path_are_not_requested() {
        for different_host in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let location = Arc::new(OnceLock::<String>::new());
            let target = location.clone();
            let image_requests = Arc::new(AtomicUsize::new(0));
            let images = image_requests.clone();
            let (origin, count) = server(move |path| {
                if path == "/doodle?app=oss" {
                    redirect(target.get().unwrap())
                } else {
                    images.fetch_add(1, Ordering::SeqCst);
                    response("200 OK", &png(1, 1))
                }
            }).await;
            let mut next = origin.clone();
            if different_host {
                next.set_host(Some("localhost")).unwrap();
                next.set_path("/doodle/a.png");
            } else {
                next.set_path("/other/a.png");
            }
            location.set(next.to_string()).unwrap();
            assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), None);
            assert_date_saved(dir.path()).await;
            assert_eq!(count.load(Ordering::SeqCst), 1);
            assert_eq!(image_requests.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn third_redirect_is_stopped_and_date_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let image_requests = Arc::new(AtomicUsize::new(0));
        let images = image_requests.clone();
        let (origin, count) = server(move |path| match path {
            "/doodle?app=oss" => redirect("/doodle/a"),
            "/doodle/a" => redirect("/doodle/b"),
            "/doodle/b" => redirect("/doodle/c.png"),
            "/doodle/c.png" => {
                images.fetch_add(1, Ordering::SeqCst);
                response("200 OK", &png(1, 1))
            }
            _ => panic!("unexpected path: {path}"),
        }).await;
        assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), None);
        assert_date_saved(dir.path()).await;
        assert_eq!(count.load(Ordering::SeqCst), 3);
        assert_eq!(image_requests.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn two_redirects_are_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let (origin, count) = server(|path| match path {
            "/doodle?app=oss" => redirect("/doodle/a?app=oss"),
            "/doodle/a?app=oss" => redirect("/doodle/b.png?app=oss"),
            "/doodle/b.png?app=oss" => response("200 OK", &png(1, 1)),
            _ => panic!("unexpected path: {path}"),
        }).await;
        let icon = dir.path().join(ICON_FILE);
        assert_eq!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.unwrap(), Some(icon.clone()));
        assert_eq!(tokio::fs::read(icon).await.unwrap(), png(1, 1));
        assert_date_saved(dir.path()).await;
        assert_eq!(count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn concurrent_syncs_share_one_request() {
        let dir = tempfile::tempdir().unwrap();
        let (origin, count) = server(|_| response("200 OK", &png(1, 1))).await;
        let client = client(&origin);
        let (first, second) = tokio::join!(
            sync(&client, origin.as_str(), dir.path(), today()),
            sync(&client, origin.as_str(), dir.path(), today()),
        );
        let expected = Some(dir.path().join(ICON_FILE));
        assert_eq!(first.unwrap(), expected);
        assert_eq!(second.unwrap(), expected);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_date_saved(dir.path()).await;
    }

    #[tokio::test]
    async fn rename_failure_cleans_temporary_file_and_does_not_save_date() {
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(dir.path().join(ICON_FILE)).await.unwrap();
        let (origin, _) = server(|_| response("200 OK", &png(1, 1))).await;
        assert!(sync(&client(&origin), origin.as_str(), dir.path(), today()).await.is_err());
        assert!(!tokio::fs::try_exists(dir.path().join(TMP_FILE)).await.unwrap());
        assert!(!tokio::fs::try_exists(dir.path().join(LAST_CHECK_FILE)).await.unwrap());
    }
}
