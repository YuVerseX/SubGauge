use super::{Hotkey, Platform, STARTUP_VALUE};
use tauri::{AppHandle, Manager};

#[cfg(windows)]
pub(super) struct System {
    hwnd: usize,
}
#[cfg(windows)]
impl System {
    pub(super) fn new(app: &AppHandle) -> Result<Self, String> {
        let window = app.get_webview_window("float").ok_or("浮窗不可用。")?;
        Ok(Self {
            hwnd: window.hwnd().map_err(|_| "无法读取浮窗句柄。")?.0 as usize,
        })
    }
}

#[cfg(windows)]
const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
#[cfg(windows)]
struct Registry(windows_sys::Win32::System::Registry::HKEY);
#[cfg(windows)]
impl Drop for Registry {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Registry::RegCloseKey(self.0);
        }
    }
}
#[cfg(windows)]
fn read_run_at(path: &str, name: &str) -> Result<Option<String>, String> {
    use windows_sys::Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*};
    let mut key = std::ptr::null_mut();
    let result = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != 0 {
        return Err("无法读取 Windows 登录启动项。".into());
    }
    let key = Registry(key);
    let mut kind = 0;
    let mut size = 0;
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if result != 0 || kind != REG_SZ || size > 65536 || size % 2 != 0 {
        return Err("Windows 登录启动项格式不可识别，未修改。".into());
    }
    let mut value = vec![0u16; size as usize / 2];
    let result = unsafe {
        RegQueryValueExW(
            key.0,
            wide(name).as_ptr(),
            std::ptr::null(),
            &mut kind,
            value.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if result != 0 || kind != REG_SZ || size as usize > value.len() * 2 || size % 2 != 0 {
        return Err("Windows 登录启动项读取失败，请重新载入。".into());
    }
    value.truncate(size as usize / 2);
    if value.last() == Some(&0) {
        value.pop();
    }
    if value.contains(&0) {
        return Err("Windows 登录启动项格式不可识别，未修改。".into());
    }
    String::from_utf16(&value)
        .map(Some)
        .map_err(|_| "Windows 登录启动项格式不可识别，未修改。".into())
}
#[cfg(windows)]
fn write_run_at(path: &str, name: &str, value: Option<&str>) -> Result<(), String> {
    use windows_sys::Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*};
    let mut key = std::ptr::null_mut();
    let result = if value.is_some() {
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        }
    } else {
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                wide(path).as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            )
        }
    };
    if value.is_none() && result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if result != 0 {
        return Err("无法修改 Windows 登录启动项。".into());
    }
    let key = Registry(key);
    let name = wide(name);
    let result = match value {
        Some(text) => {
            let value = wide(text);
            unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    REG_SZ,
                    value.as_ptr().cast(),
                    (value.len() * 2) as u32,
                )
            }
        }
        None => unsafe { RegDeleteValueW(key.0, name.as_ptr()) },
    };
    if result == 0 || (value.is_none() && result == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err("Windows 登录启动项保存失败。".into())
    }
}
#[cfg(windows)]
impl Platform for System {
    fn read_run(&self) -> Result<Option<String>, String> {
        read_run_at(RUN_KEY, STARTUP_VALUE)
    }
    fn write_run(&self, value: Option<&str>) -> Result<(), String> {
        write_run_at(RUN_KEY, STARTUP_VALUE, value)
    }
    fn startup_blocked(&self) -> Result<Option<bool>, String> {
        // StartupApproved's binary layout is not a public Windows API contract.
        // Never rewrite it or claim registration guarantees startup permission.
        Ok(None)
    }
    fn register(&self, id: i32, key: &Hotkey) -> Result<(), String> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_NOREPEAT};
        if unsafe { RegisterHotKey(self.hwnd as _, id, key.modifiers | MOD_NOREPEAT, key.key) } != 0
        {
            Ok(())
        } else {
            Err("快捷键已被占用或无法注册，请换一个组合；原快捷键保持不变。".into())
        }
    }
    fn unregister(&self, id: i32) -> Result<(), String> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey;
        if unsafe { UnregisterHotKey(self.hwnd as _, id) } != 0 {
            Ok(())
        } else {
            Err("旧快捷键未能释放，请重启应用。".into())
        }
    }
}
#[cfg(windows)]
pub(super) fn open_startup_settings() -> Result<(), String> {
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL};
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide("ms-settings:startupapps").as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if result > 32 {
        Ok(())
    } else {
        Err("无法打开 Windows 启动应用设置，请在系统设置中手动打开。".into())
    }
}

#[cfg(not(windows))]
pub(super) struct System;
#[cfg(not(windows))]
impl System {
    pub(super) fn new(_app: &AppHandle) -> Result<Self, String> {
        Ok(Self)
    }
}
#[cfg(not(windows))]
impl Platform for System {
    fn read_run(&self) -> Result<Option<String>, String> {
        Err("此功能仅支持 Windows。".into())
    }
    fn write_run(&self, _value: Option<&str>) -> Result<(), String> {
        Err("此功能仅支持 Windows。".into())
    }
    fn startup_blocked(&self) -> Result<Option<bool>, String> {
        Ok(None)
    }
    fn register(&self, _id: i32, _key: &Hotkey) -> Result<(), String> {
        Err("此功能仅支持 Windows。".into())
    }
    fn unregister(&self, _id: i32) -> Result<(), String> {
        Ok(())
    }
}
#[cfg(not(windows))]
pub(super) fn open_startup_settings() -> Result<(), String> {
    Err("此功能仅支持 Windows。".into())
}

#[cfg(all(test, windows))]
#[path = "native_tests.rs"]
mod tests;
