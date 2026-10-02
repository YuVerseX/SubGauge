use std::{fs, path::Path};

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("配置目录无效")?;
    fs::create_dir_all(dir).map_err(|_| "无法创建配置目录")?;
    let temp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|_| "无法写入配置")?;
        f.write_all(bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "无法保存配置")?;
        drop(f);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let target: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                if MoveFileExW(source.as_ptr(), target.as_ptr(), 1 | 8) == 0 {
                    return Err("无法原子替换配置".into());
                }
            }
        }
        #[cfg(not(windows))]
        fs::rename(&temp, path).map_err(|_| "无法原子替换配置")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
#[cfg(windows)]
#[repr(C)]
struct Blob {
    len: u32,
    data: *mut u8,
}
#[cfg(windows)]
#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *const Blob,
        description: *const u16,
        entropy: *const Blob,
        reserved: *mut std::ffi::c_void,
        prompt: *mut std::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *mut std::ffi::c_void,
        prompt: *mut std::ffi::c_void,
        flags: u32,
        output: *mut Blob,
    ) -> i32;
}
#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
}
#[cfg(windows)]
fn crypt(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    if bytes.len() > u32::MAX as usize {
        return Err("会话数据过大".into());
    }
    let input = Blob {
        len: bytes.len() as u32,
        data: bytes.as_ptr() as *mut u8,
    };
    let mut output = Blob {
        len: 0,
        data: std::ptr::null_mut(),
    };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut output,
            )
        }
    };
    if ok == 0 {
        return Err("Windows 安全存储不可用，请重新登录。".into());
    }
    let result = unsafe { std::slice::from_raw_parts(output.data, output.len as usize).to_vec() };
    unsafe {
        std::ptr::write_bytes(output.data, 0, output.len as usize);
        LocalFree(output.data.cast());
    }
    Ok(result)
}
pub fn protect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        crypt(bytes, true)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err("记住登录仅支持 Windows 安全存储。".into())
    }
}
pub fn unprotect(bytes: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        crypt(bytes, false)
    }
    #[cfg(not(windows))]
    {
        let _ = bytes;
        Err("安全存储仅支持 Windows。".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn dpapi_roundtrip() {
        let protected = super::protect(b"test-session-only").unwrap();
        assert_ne!(protected, b"test-session-only");
        assert_eq!(super::unprotect(&protected).unwrap(), b"test-session-only");
    }
}
