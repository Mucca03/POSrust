use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::Context;
use pos_hardware::transport;

#[derive(Clone)]
pub(crate) enum Destination {
    Files(PathBuf),
    Tcp(String),
    #[cfg(unix)]
    Cups(String),
    #[cfg(windows)]
    WindowsSpool(String),
}

impl Destination {
    pub fn parse(value: &str) -> Result<Self, String> {
        let (scheme, address) = value.split_once(':').ok_or("falta el destino de impresión")?;
        let address = address.trim();
        if address.is_empty() {
            return Err("el destino de impresión está vacío".to_owned());
        }
        match scheme {
            "file" => Ok(Self::Files(address.into())),
            "tcp" => {
                let (host, port) = address.rsplit_once(':').ok_or("use tcp:<host:puerto>")?;
                if host.is_empty() || port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
                    return Err("use tcp:<host:puerto>".into());
                }
                Ok(Self::Tcp(address.to_owned()))
            }
            #[cfg(unix)]
            "cups" if !address.starts_with('-') && !address.chars().any(char::is_control) => {
                Ok(Self::Cups(address.to_owned()))
            }
            #[cfg(windows)]
            "windows_spool" if !address.chars().any(char::is_control) => {
                Ok(Self::WindowsSpool(address.to_owned()))
            }
            _ => Err(format!("destino de impresión no válido en este sistema: {scheme}")),
        }
    }

    pub fn supports_drawer(&self) -> bool {
        !matches!(self, Self::Files(_))
    }

    pub fn label(&self) -> String {
        match self {
            Self::Files(dir) => format!("Archivo · {}", dir.display()),
            Self::Tcp(address) => format!("TCP · {address}"),
            #[cfg(unix)]
            Self::Cups(queue) => format!("CUPS · {queue}"),
            #[cfg(windows)]
            Self::WindowsSpool(queue) => format!("Windows RAW · {queue}"),
        }
    }

    fn send(&self, job: &Job, sequence: u64) -> io::Result<String> {
        match self {
            Self::Files(dir) => {
                std::fs::create_dir_all(dir)?;
                let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?
                    .as_nanos();
                let file = dir.join(format!("{}-{stamp}-{}-{sequence}.bin", job.filename, std::process::id()));
                transport::to_file(&file, &job.bytes)?;
                Ok(format!("Archivo de prueba: {}", file.display()))
            }
            Self::Tcp(address) => {
                transport::to_tcp(address.as_str(), &job.bytes, Duration::from_secs(3))?;
                Ok("Enviado a la impresora de red".into())
            }
            #[cfg(unix)]
            Self::Cups(queue) => {
                transport::to_cups(queue, &job.bytes)?;
                Ok(format!("Enviado a la cola {queue}"))
            }
            #[cfg(windows)]
            Self::WindowsSpool(queue) => {
                transport::to_windows_spool(queue, &job.bytes)?;
                Ok(format!("Enviado a la cola RAW {queue}"))
            }
        }
    }
}

pub(crate) struct Job {
    pub destination: Destination,
    pub filename: String,
    pub audit_key: String,
    pub user_id: String,
    pub receipt: bool,
    pub bytes: Vec<u8>,
}

pub(crate) struct Outcome {
    pub job: Job,
    pub result: io::Result<String>,
}

pub(crate) struct Printer {
    tx: SyncSender<Job>,
    pub done: Receiver<Outcome>,
}

impl Printer {
    pub fn start(context: Context) -> io::Result<Self> {
        let (tx, pending) = mpsc::sync_channel::<Job>(8);
        let (finished, done) = mpsc::channel();
        std::thread::Builder::new().name("pos-print".into()).spawn(move || {
            for (index, job) in pending.into_iter().enumerate() {
                let result = job.destination.send(&job, index as u64);
                if finished.send(Outcome { job, result }).is_err() {
                    break;
                }
                context.request_repaint();
            }
        })?;
        Ok(Self { tx, done })
    }

    pub fn submit(&self, job: Job) -> Result<(), String> {
        self.tx.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => "la cola está llena; vuelve a intentar manualmente".into(),
            TrySendError::Disconnected(_) => "la cola de impresión no está disponible".into(),
        })
    }
}
