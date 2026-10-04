use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

const IDLE: u8 = 0;
const PENDING: u8 = 1;
const ACTIVE: u8 = 2;
const FINISHING: u8 = 3;

#[derive(Clone, Copy)]
pub(super) struct SystemGesture {
    pub rect: super::Rect,
    pub size: tauri::PhysicalSize<u32>,
    pub scale: f64,
    pub direction: Option<&'static str>,
}

#[derive(Default)]
pub(super) struct Control {
    generation: AtomicU64,
    phase: AtomicU8,
    ready: AtomicBool,
    recovery_pending: AtomicBool,
    implicit: Mutex<Option<SystemGesture>>,
}

impl Control {
    pub(super) fn next_generation(&self) -> u64 {
        self.generation
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
    }
    pub(super) fn take_system_gesture(&self) -> Option<(u64, SystemGesture)> {
        self.implicit
            .try_lock()
            .ok()?
            .take()
            .map(|gesture| (self.generation.load(Ordering::Acquire), gesture))
    }
    pub(super) fn begin(&self, generation: u64) {
        self.generation.store(generation, Ordering::Release);
        self.phase.store(PENDING, Ordering::Release);
    }
    pub(super) fn active(&self) -> bool {
        self.phase.load(Ordering::Acquire) != IDLE
    }
    pub(super) fn entered(&self) {
        let _ = self
            .phase
            .compare_exchange(PENDING, ACTIVE, Ordering::AcqRel, Ordering::Acquire);
    }
    pub(super) fn request_end(&self) -> Option<u64> {
        let phase = self.phase.load(Ordering::Acquire);
        if phase != PENDING && phase != ACTIVE {
            return None;
        }
        self.phase
            .compare_exchange(phase, FINISHING, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        Some(self.generation.load(Ordering::Acquire))
    }
    pub(super) fn finish(&self, generation: u64) {
        if self.generation.load(Ordering::Acquire) == generation {
            self.phase.store(IDLE, Ordering::Release);
        }
    }
    pub(super) fn pending(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
            && self.phase.load(Ordering::Acquire) == PENDING
    }
    pub(super) fn ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
}

// Wry runs run_on_main_thread inline when called on that thread. Dispatch via a
// worker so the native callback always returns before we query/mutate windows.
pub(super) fn schedule(app: AppHandle, task: impl FnOnce(AppHandle) + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        let target = app.clone();
        let _ = app.run_on_main_thread(move || task(target));
    });
}

#[cfg(windows)]
struct Hook {
    app: AppHandle,
    control: Arc<Control>,
}

#[cfg(windows)]
pub(super) fn capture_system_gesture(
    hwnd: windows_sys::Win32::Foundation::HWND,
    hit: usize,
    control: &Control,
) {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                GetClientRect, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION,
                HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
            },
        },
    };
    if control.active() {
        return;
    }
    let direction = match hit as u32 {
        HTLEFT => Some("West"),
        HTRIGHT => Some("East"),
        HTTOP => Some("North"),
        HTBOTTOM => Some("South"),
        HTTOPLEFT => Some("NorthWest"),
        HTTOPRIGHT => Some("NorthEast"),
        HTBOTTOMLEFT => Some("SouthWest"),
        HTBOTTOMRIGHT => Some("SouthEast"),
        HTCAPTION => None,
        _ => return,
    };
    let mut rect = RECT::default();
    let mut client = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0
        || unsafe { GetClientRect(hwnd, &mut client) } == 0
    {
        return;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let Some(scale) = (dpi != 0).then_some(f64::from(dpi) / 96.0) else {
        return;
    };
    // Direct HWND reads cannot wait on the framework event loop. This metadata
    // lock is nonblocking and released before forwarding any native message.
    let Ok(mut implicit) = control.implicit.try_lock() else {
        return;
    };
    *implicit = Some(SystemGesture {
        rect: super::Rect {
            x: rect.left,
            y: rect.top,
            width: (rect.right - rect.left).max(1) as u32,
            height: (rect.bottom - rect.top).max(1) as u32,
        },
        size: tauri::PhysicalSize::new(
            (client.right - client.left).max(1) as u32,
            (client.bottom - client.top).max(1) as u32,
        ),
        scale,
        direction,
    });
    let generation = control.next_generation();
    control.begin(generation);
}

#[cfg(windows)]
unsafe extern "system" fn callback(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    id: usize,
    reference: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
        HTTOPRIGHT, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMECRITICAL, PBT_APMRESUMESUSPEND, SC_MOVE,
        SC_SIZE, WM_DISPLAYCHANGE, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY,
        WM_NCLBUTTONDOWN, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SYSCOMMAND,
    };
    // The hook owns exactly one Box until WM_NCDESTROY; all work holds its own
    // AppHandle/Arc copies and never retains this pointer.
    let hook = unsafe { &*(reference as *const Hook) };
    match message {
        windows_sys::Win32::UI::WindowsAndMessaging::WM_HOTKEY => {
            let hotkey_id = wparam as i32;
            schedule(hook.app.clone(), move |app| {
                crate::desktop::hotkey(&app, hotkey_id);
            });
        }
        WM_SYSCOMMAND
            if (wparam as u32 & 0xfff0) == SC_SIZE || (wparam as u32 & 0xfff0) == SC_MOVE =>
        {
            let hit = if (wparam as u32 & 0xfff0) == SC_MOVE {
                HTCAPTION
            } else {
                match wparam as u32 & 0x000f {
                    1 => HTLEFT,
                    2 => HTRIGHT,
                    3 => HTTOP,
                    4 => HTTOPLEFT,
                    5 => HTTOPRIGHT,
                    6 => HTBOTTOM,
                    7 => HTBOTTOMLEFT,
                    _ => HTBOTTOMRIGHT,
                }
            };
            let was_active = hook.control.active();
            capture_system_gesture(hwnd, hit as usize, &hook.control);
            if !was_active && hook.control.active() {
                super::check_gesture_start(
                    hook.app.clone(),
                    hook.control.generation.load(Ordering::Acquire),
                );
            }
        }
        WM_NCLBUTTONDOWN => {
            let was_active = hook.control.active();
            capture_system_gesture(hwnd, wparam, &hook.control);
            if !was_active && hook.control.active() {
                super::check_gesture_start(
                    hook.app.clone(),
                    hook.control.generation.load(Ordering::Acquire),
                );
            }
        }
        WM_ENTERSIZEMOVE => {
            capture_system_gesture(hwnd, HTCAPTION as usize, &hook.control);
            hook.control.entered();
        }
        WM_EXITSIZEMOVE | WM_SHOWWINDOW if message == WM_EXITSIZEMOVE || wparam == 0 => {
            if let Some(generation) = hook.control.request_end() {
                schedule(hook.app.clone(), move |app| {
                    app.state::<super::Windows>()
                        .finish_gesture(&app, generation)
                });
            }
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_POWERBROADCAST
            if message != WM_POWERBROADCAST
                || [
                    PBT_APMRESUMEAUTOMATIC,
                    PBT_APMRESUMESUSPEND,
                    PBT_APMRESUMECRITICAL,
                ]
                .contains(&(wparam as u32)) =>
        {
            if !hook.control.recovery_pending.swap(true, Ordering::AcqRel) {
                let control = hook.control.clone();
                schedule(hook.app.clone(), move |app| {
                    control.recovery_pending.store(false, Ordering::Release);
                    app.state::<super::Windows>().recover(&app);
                });
            }
        }
        WM_NCDESTROY => {
            if let Some(desktop) = hook.app.try_state::<crate::desktop::Desktop>() {
                desktop.release();
            }
            unsafe {
                RemoveWindowSubclass(hwnd, Some(callback), id);
            }
            hook.control.ready.store(false, Ordering::Release);
            hook.control.phase.store(IDLE, Ordering::Release);
            unsafe {
                drop(Box::from_raw(reference as *mut Hook));
            }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(windows)]
pub(super) fn install(app: &AppHandle, control: Arc<Control>) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
    let hwnd = window.hwnd().map_err(|_| "无法读取浮窗句柄")?;
    let reference = Box::into_raw(Box::new(Hook {
        app: app.clone(),
        control: control.clone(),
    }));
    // Called by setup on the HWND's owning thread, never by an IPC worker.
    if unsafe { SetWindowSubclass(hwnd.0, Some(callback), 0x5355_4247, reference as usize) } == 0 {
        unsafe {
            drop(Box::from_raw(reference));
        }
        return Err("无法初始化浮窗移动监听".into());
    }
    control.ready.store(true, Ordering::Release);
    Ok(())
}

#[cfg(windows)]
pub(super) fn pointer() -> Option<(i32, i32)> {
    use windows_sys::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};
    let mut point = POINT::default();
    (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x, point.y))
}

#[cfg(windows)]
pub(super) fn topmost(window: &tauri::WebviewWindow) -> Result<bool, String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, IsWindow, GWL_EXSTYLE, WS_EX_TOPMOST,
    };
    let hwnd = window.hwnd().map_err(|_| "无法读取浮窗句柄")?;
    if unsafe { IsWindow(hwnd.0) } == 0 {
        return Err("浮窗已关闭".into());
    }
    Ok(unsafe { GetWindowLongPtrW(hwnd.0, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0)
}

#[cfg(not(windows))]
pub(super) fn install(_app: &AppHandle, _control: Arc<Control>) -> Result<(), String> {
    Err("此版本的窗口手势仅支持 Windows".into())
}
#[cfg(not(windows))]
pub(super) fn pointer() -> Option<(i32, i32)> {
    None
}
#[cfg(not(windows))]
pub(super) fn topmost(window: &tauri::WebviewWindow) -> Result<bool, String> {
    window
        .is_always_on_top()
        .map_err(|_| "无法读取浮窗置顶状态".into())
}
