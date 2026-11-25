use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use image::{ImageFormat, ImageReader, Limits};

use super::beside_database;

pub(super) fn local_path(db: &Path, value: &str) -> PathBuf {
    beside_database(db, value.trim())
}

pub(super) fn load_logo(ctx: &Context, db: &Path, value: &str) -> Result<Option<TextureHandle>, Box<dyn Error>> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let bytes = read_small_file(&local_path(db, value), 4 * 1024 * 1024)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
    let mut limits = Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode()?.thumbnail(512, 256).into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(Some(ctx.load_texture("login-brand", ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        TextureOptions::LINEAR)))
}

pub(super) fn load_text(db: &Path, value: &str) -> Result<String, Box<dyn Error>> {
    if value.trim().is_empty() { return Err("escribe la ruta del archivo de texto".into()); }
    let bytes = read_small_file(&local_path(db, value), 4096)?;
    let text = std::str::from_utf8(&bytes)?;
    Ok(text.trim_start_matches('\u{feff}').trim().to_owned())
}

fn read_small_file(path: &Path, limit: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    if !path.metadata()?.is_file() { return Err("la ruta debe apuntar a un archivo normal".into()); }
    let mut bytes = Vec::new();
    File::open(path)?.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit { return Err("el archivo supera el tamaño permitido".into()); }
    Ok(bytes)
}
