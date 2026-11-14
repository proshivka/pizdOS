use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub fn fetch_to_file(repo_base: &str, rel_name: &str, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    if let Some(local_dir) = repo_base.strip_prefix("file://") {
        let src = Path::new(local_dir).join(rel_name);
        fs::copy(&src, dest)?;
        return Ok(());
    }

    if !repo_base.starts_with("http://") && !repo_base.starts_with("https://") {
        let src = Path::new(repo_base).join(rel_name);
        fs::copy(&src, dest)?;
        return Ok(());
    }

    let url = format!(
        "{}/{}",
        repo_base.trim_end_matches('/'),
        rel_name.trim_start_matches('/')
    );

    if url.starts_with("http://") {
        http_get_file(&url, dest, 5)
    } else {
        fetch_https_cli(&url, dest)
    }
}

fn fetch_https_cli(url: &str, dest: &Path) -> io::Result<()> {
    if let Ok(status) = Command::new("wget").arg("-q").arg("-O").arg(dest).arg(url).status()
        && status.success()
    {
        return Ok(());
    }
    if let Ok(status) = Command::new("curl").arg("-fsSL").arg("-o").arg(dest).arg(url).status()
        && status.success()
    {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "failed to fetch {url} (https requires BusyBox wget or curl)"
    )))
}

fn http_get_file(url: &str, dest: &Path, redirects_left: u8) -> io::Result<()> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "expected http:// URL"))?;
    let (host_port, path) = match rest.split_once('/') {
        Some((hp, p)) => (hp, format!("/{p}")),
        None => (rest, String::from("/")),
    };
    let addr = if host_port.contains(':') {
        host_port.to_string()
    } else {
        format!("{host_port}:80")
    };

    let mut stream = TcpStream::connect(&addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;

    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host_port}\r\nUser-Agent: piz-get/0.1\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(req.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid HTTP status line"))?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;
    let mut location: Option<String> = None;

    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let k_lower = k.trim().to_ascii_lowercase();
            let v_trim = v.trim();
            match k_lower.as_str() {
                "content-length" => content_length = v_trim.parse().ok(),
                "transfer-encoding" => {
                    if v_trim.to_ascii_lowercase().contains("chunked") {
                        chunked = true;
                    }
                }
                "location" => location = Some(v_trim.to_string()),
                _ => {}
            }
        }
    }

    if (300..400).contains(&status_code)
        && let Some(loc) = location
    {
        if redirects_left == 0 {
            return Err(io::Error::other("too many HTTP redirects"));
        }
        if loc.starts_with("https://") {
            return fetch_https_cli(&loc, dest);
        }
        return http_get_file(&loc, dest, redirects_left - 1);
    }

    if status_code != 200 {
        return Err(io::Error::other(format!(
            "HTTP {status_code} while fetching {url}"
        )));
    }

    let mut out = File::create(dest)?;
    if chunked {
        decode_chunked(&mut reader, &mut out)?;
    } else if let Some(len) = content_length {
        let mut limited = reader.take(len as u64);
        io::copy(&mut limited, &mut out)?;
    } else {
        io::copy(&mut reader, &mut out)?;
    }
    out.flush()?;
    Ok(())
}

fn decode_chunked<R: BufRead, W: Write>(reader: &mut R, out: &mut W) -> io::Result<()> {
    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line)?;
        let hex_part = size_line.trim().split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(hex_part, 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if size == 0 {
            break;
        }
        let mut chunk = vec![0u8; size];
        reader.read_exact(&mut chunk)?;
        out.write_all(&chunk)?;

        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
    }
    Ok(())
}
