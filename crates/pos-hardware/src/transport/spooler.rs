use std::{io, ptr};

use windows_sys::Win32::Graphics::Printing::{
    AbortPrinter, ClosePrinter, EndDocPrinter, EndPagePrinter, OpenPrinterW, StartDocPrinterW,
    StartPagePrinter, WritePrinter, DOC_INFO_1W, PRINTER_HANDLE,
};

struct Printer(PRINTER_HANDLE);

impl Drop for Printer {
    fn drop(&mut self) {
        // SAFETY: this guard owns the handle returned by OpenPrinterW.
        unsafe { ClosePrinter(self.0) };
    }
}

/// Sends already encoded ESC/POS bytes through the Windows RAW spooler.
/// Success means the job was accepted by the spooler, not that it printed.
pub fn to_windows_spool(queue: &str, bytes: &[u8]) -> io::Result<()> {
    if queue.trim().is_empty() || queue.chars().any(char::is_control) || bytes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "cola RAW o trabajo inválido"));
    }
    let size = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "trabajo RAW muy grande"))?;
    let name: Vec<_> = queue.encode_utf16().chain([0]).collect();
    let mut handle = PRINTER_HANDLE::default();
    // SAFETY: name is NUL-terminated, handle points to writable storage, and
    // NULL selects the default PRINTER_DEFAULTS.
    if unsafe { OpenPrinterW(name.as_ptr(), &mut handle, ptr::null()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let printer = Printer(handle);
    let mut document: Vec<_> = "POSrust ESC/POS".encode_utf16().chain([0]).collect();
    let mut raw = [b'R' as u16, b'A' as u16, b'W' as u16, 0];
    let info = DOC_INFO_1W {
        pDocName: document.as_mut_ptr(),
        pOutputFile: ptr::null_mut(),
        pDatatype: raw.as_mut_ptr(),
    };
    // SAFETY: the owned handle is valid and both UTF-16 strings outlive the
    // call. Level 1 expects DOC_INFO_1W.
    if unsafe { StartDocPrinterW(printer.0, 1, &info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut written = 0;
        // SAFETY: this handle has a started document; the input buffer has
        // exactly `size` initialized bytes, and written points to a u32.
        unsafe {
            if StartPagePrinter(printer.0) == 0
                || WritePrinter(printer.0, bytes.as_ptr().cast(), size, &mut written) == 0
            {
                return Err(io::Error::last_os_error());
            }
            if written != size {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "envío RAW incompleto"));
            }
            if EndPagePrinter(printer.0) == 0 || EndDocPrinter(printer.0) == 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    })();
    if result.is_err() {
        // SAFETY: try to cancel only the failed document on this owned handle.
        unsafe { AbortPrinter(printer.0) };
    }
    result
}
