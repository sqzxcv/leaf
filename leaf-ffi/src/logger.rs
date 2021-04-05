use std::io::{self, Write};

use bytes::BytesMut;
use log::{Level, Metadata, Record};

#[cfg(target_os = "ios")]
mod platform_log {
    pub fn log_out(data: &[u8]) {
        use crate::ios::{asl_log, ASL_LEVEL_NOTICE};
        use std::{ffi, ptr};
        unsafe {
            let s = match ffi::CString::new(data) {
                Ok(s) => s,
                Err(_) => return,
            };
            asl_log(
                ptr::null_mut(),
                ptr::null_mut(),
                ASL_LEVEL_NOTICE as i32,
                // ffi::CString::new("%s").unwrap().as_c_str().as_ptr(),
                s.as_c_str().as_ptr(),
            )
        };
    }

    pub fn log_text(text: &str) {
        log_out(text.as_bytes());
    }
}

#[cfg(target_os = "android")]
mod platform_log {
    use super::bindings::{__android_log_print, android_LogPriority_ANDROID_LOG_VERBOSE};
    pub fn log_out(data: &[u8]) {
        unsafe {
            let s = match std::ffi::CString::new(data) {
                Ok(s) => s,
                Err(_) => return,
            };
            let _ = __android_log_print(
                android_LogPriority_ANDROID_LOG_VERBOSE as std::os::raw::c_int,
                "leaf".as_ptr() as _,
                s.as_c_str().as_ptr(),
            );
        }
    }

    pub fn log_text(text: &str) {
        log_out(text.as_bytes());
    }
}

#[cfg(target_os = "windows")]
mod platform_log {
    extern "system" {
        fn OutputDebugStringW(lp_output_string: *const u16);
        fn GetTickCount() -> u64;
    }
    pub fn log_text(text: &str) {
        use std::{ffi::OsStr, os::windows::prelude::OsStrExt};
        let mut bytes: Vec<_> =
            unsafe { OsStr::new(GetTickCount().to_string().as_str()).encode_wide() }.collect(); //Vec::with_capacity(text.len() + 12);
        bytes.reserve(text.len() + 12);
        bytes.extend(OsStr::new(text).encode_wide());
        bytes.extend_from_slice(&[13, 10, 0]);
        unsafe { OutputDebugStringW(bytes.as_ptr()) };
    }
    pub fn log_out(data: &[u8]) {
        log_text(String::from_utf8_lossy(data).as_ref());
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android", target_os = "windows")))]
mod platform_log {
    fn log_out(_data: &[u8]) {}
    fn log_text(_text: &str) {}
}

pub struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            platform_log::log_text(
                format!(
                    "[{}] [{}] {}",
                    record.level(),
                    record.target(),
                    record.args()
                )
                .as_str(),
            )
        }
    }
}

pub struct ConsoleWriter(pub BytesMut);

impl Default for ConsoleWriter {
    fn default() -> Self {
        ConsoleWriter(BytesMut::new())
    }
}

unsafe impl Send for ConsoleWriter {}

impl Write for ConsoleWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(buf);
        if let Some(i) = memchr::memchr(b'\n', &self.0) {
            platform_log::log_out(&self.0[..i]);
            let _ = self.0.split_to(i + 1);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
