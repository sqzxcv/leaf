use std::{
    ffi::OsStr,
    fs::File,
    io::Error,
    os::windows::{ffi::OsStrExt, io::FromRawHandle},
    ptr::null_mut,
};
use winapi::um::{
    fileapi::{CreateFile2, OPEN_EXISTING},
    handleapi::INVALID_HANDLE_VALUE,
    winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ},
};

pub fn unrolled_find_u16s(needle: u16, haystack: &[u16]) -> Option<usize> {
    let ptr = haystack.as_ptr();
    let mut start = &haystack[..];

    // For performance reasons unfold the loop eight times.
    while start.len() >= 8 {
        macro_rules! if_return {
            ($($n:literal,)+) => {
                $(
                    if start[$n] == needle {
                        return Some((&start[$n] as *const u16 as usize - ptr as usize) / 2);
                    }
                )+
            }
        }

        if_return!(0, 1, 2, 3, 4, 5, 6, 7,);

        start = &start[8..];
    }

    for c in start {
        if *c == needle {
            return Some((c as *const u16 as usize - ptr as usize) / 2);
        }
    }
    None
}

pub fn to_u16s<S: AsRef<OsStr>>(s: S) -> std::io::Result<Vec<u16>> {
    fn inner(s: &OsStr) -> std::io::Result<Vec<u16>> {
        let mut maybe_result: Vec<u16> = s.encode_wide().collect();
        if unrolled_find_u16s(0, &maybe_result).is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "strings passed to WinAPI cannot contain NULs",
            ));
        }
        maybe_result.push(0);
        Ok(maybe_result)
    }
    inner(s.as_ref())
}

pub fn open_file(path: impl AsRef<OsStr>) -> Result<File, Error> {
    let path = to_u16s(path)?;
    let handle = unsafe {
        CreateFile2(
            path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            OPEN_EXISTING,
            null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        Err(Error::last_os_error())
    } else {
        Ok(unsafe { File::from_raw_handle(handle) })
    }
}
