//! Trabajos RAW. No programa reintentos: repetir un envío puede duplicar una
//! tirilla o un pulso si la impresora recibió bytes antes de fallar.

use std::fs::File;
use std::io::{self, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

#[cfg(windows)]
#[allow(unsafe_code)]
mod spooler;
#[cfg(windows)]
pub use spooler::to_windows_spool;

pub fn to_file(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut file = File::create_new(path)?;
    file.write_all(data)?;
    file.sync_all()
}

pub fn to_tcp(addr: impl ToSocketAddrs, data: &[u8], timeout: Duration) -> io::Result<()> {
    let mut last_error = None;
    for socket in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&socket, timeout) {
            Ok(mut stream) => {
                stream.set_write_timeout(Some(timeout))?;
                stream.write_all(data)?;
                return stream.shutdown(std::net::Shutdown::Write);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "dirección sin IP")))
}

#[cfg(unix)]
pub fn to_cups(queue: &str, data: &[u8]) -> io::Result<()> {
    use std::process::{Command, Stdio};

    if queue.is_empty() || queue.starts_with('-') || queue.chars().any(char::is_control) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "cola CUPS inválida"));
    }
    let mut process = Command::new("lp")
        .args(["-d", queue, "-o", "raw", "-"])
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
    let write = process.stdin.take().expect("stdin solicitado").write_all(data);
    let output = process.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("CUPS: {}", String::from_utf8_lossy(&output.stderr).trim())));
    }
    write
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpListener;

    #[test]
    fn envia_bytes_sin_transformarlos() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let data = b"\x1b@\xa4\n";
        to_tcp(listener.local_addr().unwrap(), data, Duration::from_secs(2)).unwrap();
        let (mut remote, _) = listener.accept().unwrap();
        remote.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut received = Vec::new();
        remote.read_to_end(&mut received).unwrap();
        assert_eq!(data.as_slice(), received);
    }
}
