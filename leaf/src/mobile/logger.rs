use std::io::{self, Write};

use bytes::BytesMut;

#[cfg(target_os = "ios")]
mod platform_log {
    pub fn log_out(data: &[u8]) {
        use super::bindings::{asl_log, ASL_LEVEL_NOTICE};
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
}

#[cfg(target_os = "windows")]
mod platform_log {
    extern "system" {
        fn OutputDebugStringW(lp_output_string: *const u16);
    }
    pub fn log_text(text: &str) {
        use std::{ffi::OsStr, os::windows::prelude::OsStrExt};
        let mut bytes: Vec<_> = OsStr::new(text).encode_wide().collect();
        bytes.push(0);
        unsafe { OutputDebugStringW(bytes.as_ptr()) };
    }
    pub fn log_out(data: &[u8]) {
        log_text(String::from_utf8_lossy(data).as_ref());
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android", target_os = "windows")))]
mod platform_log {
    fn log_out(_data: &[u8]) {}
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
        self.0.clear();
        self.0.extend_from_slice(buf);
        #[cfg(target_vendor = "uwp")]
        platform_log::log_out(&self.0[..]);
        #[cfg(not(target_vendor = "uwp"))]
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
