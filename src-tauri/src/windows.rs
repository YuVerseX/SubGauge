use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, PhysicalUnit, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowSizeConstraints,
};

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;

mod native;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}
impl Rect {
    fn intersection(self, other: Rect) -> u64 {
        let width = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width))
            - i64::from(self.x.max(other.x));
        let height = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height))
            - i64::from(self.y.max(other.y));
        width.max(0) as u64 * height.max(0) as u64
    }
    fn contains(self, point: (i32, i32)) -> bool {
        i64::from(point.0) >= i64::from(self.x)
            && i64::from(point.0) < i64::from(self.x) + i64::from(self.width)
            && i64::from(point.1) >= i64::from(self.y)
            && i64::from(point.1) < i64::from(self.y) + i64::from(self.height)
    }
    fn distance(self, other: Rect) -> u64 {
        let dx = (i64::from(self.x) - i64::from(other.x) - i64::from(other.width))
            .max(i64::from(other.x) - i64::from(self.x) - i64::from(self.width))
            .max(0) as u64;
        let dy = (i64::from(self.y) - i64::from(other.y) - i64::from(other.height))
            .max(i64::from(other.y) - i64::from(self.y) - i64::from(self.height))
            .max(0) as u64;
        dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
    }
    fn clamped(self, area: Rect) -> Self {
        let max_x = area
            .x
            .saturating_add(area.width as i32)
            .saturating_sub(self.width as i32)
            .max(area.x);
        let max_y = area
            .y
            .saturating_add(area.height as i32)
            .saturating_sub(self.height as i32)
            .max(area.y);
        Self {
            x: self.x.clamp(area.x, max_x),
            y: self.y.clamp(area.y, max_y),
            ..self
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Screen {
    bounds: Rect,
    work: Rect,
    scale: f64,
}

fn select_screen(
    screens: &[Screen],
    rect: Rect,
    pointer: Option<(i32, i32)>,
    origin: Option<Rect>,
) -> Option<Screen> {
    screens.iter().copied().max_by_key(|screen| {
        let area = rect.intersection(screen.bounds);
        (
            area,
            std::cmp::Reverse(if area == 0 {
                rect.distance(screen.bounds)
            } else {
                0
            }),
            pointer.is_some_and(|point| screen.bounds.contains(point)),
            origin == Some(screen.bounds),
            std::cmp::Reverse((screen.bounds.x, screen.bounds.y)),
        )
    })
}

fn screens(window: &WebviewWindow) -> Vec<Screen> {
    window
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|monitor| {
            let area = monitor.work_area();
            Screen {
                bounds: Rect {
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width,
                    height: monitor.size().height,
                },
                work: Rect {
                    x: area.position.x,
                    y: area.position.y,
                    width: area.size.width,
                    height: area.size.height,
                },
                scale: monitor.scale_factor(),
            }
        })
        .collect()
}
const CONFIG_VERSION: u32 = 1;
const DEFAULT_WIDTH: f64 = 316.0;
const MIN_WIDTH: f64 = 280.0;
const MAX_WIDTH: f64 = 640.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModeSize {
    width: f64,
    height: Option<f64>,
}
impl Default for ModeSize {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: None,
        }
    }
}
impl ModeSize {
    fn normalize(&mut self) {
        self.width = if self.width.is_finite() {
            self.width.clamp(MIN_WIDTH, MAX_WIDTH)
        } else {
            DEFAULT_WIDTH
        };
        self.height = self.height.filter(|h| h.is_finite() && *h >= 1.0);
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct SavedPosition {
    x: i32,
    y: i32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WindowConfig {
    version: u32,
    position: Option<SavedPosition>,
    compact: ModeSize,
    expanded: ModeSize,
}
impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            position: None,
            compact: ModeSize::default(),
            expanded: ModeSize::default(),
        }
    }
}
impl WindowConfig {
    fn mode(&self, expanded: bool) -> ModeSize {
        if expanded {
            self.expanded
        } else {
            self.compact
        }
    }
    fn mode_mut(&mut self, expanded: bool) -> &mut ModeSize {
        if expanded {
            &mut self.expanded
        } else {
            &mut self.compact
        }
    }
    fn decode(bytes: &[u8]) -> Result<Self, ()> {
        let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| ())?;
        if value.get("version").is_some() {
            // Do not destroy a newer application's settings when running an older build.
            if value.get("version").and_then(|v| v.as_u64()) != Some(CONFIG_VERSION.into()) {
                return Err(());
            }
            let mut config: Self = serde_json::from_value(value).map_err(|_| ())?;
            config.compact.normalize();
            config.expanded.normalize();
            Ok(config)
        } else {
            let rect: Rect = serde_json::from_value(value).map_err(|_| ())?;
            // Old dimensions were content-driven physical pixels, never a manual preference.
            Ok(Self {
                position: Some(SavedPosition {
                    x: rect.x,
                    y: rect.y,
                }),
                ..Self::default()
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FloatSizeState {
    pub width: f64,
    pub height: f64,
    pub manual_height: bool,
    pub resizing: bool,
}

#[derive(Clone, Copy)]
struct ManualResize {
    generation: u64,
    horizontal: bool,
    vertical: bool,
    start: PhysicalSize<u32>,
    changed_width: bool,
    changed_height: bool,
}
#[derive(Clone, Copy)]
struct ManualMove {
    generation: u64,
    start: Rect,
    scale: f64,
    origin: Option<Rect>,
}
#[derive(Clone, Copy)]
struct LayoutRequest {
    height: f64,
    min_height: f64,
    expanded: bool,
    menu_open: bool,
    expected_generation: Option<u64>,
}
#[derive(Clone)]
struct Layout {
    collapsed: Option<Rect>,
    expanded: bool,
    menu: Option<Rect>,
    pending_restore: Option<Rect>,
    config: WindowConfig,
    writable: bool,
    natural_height: f64,
    min_height: f64,
    resize: Option<ManualResize>,
    movement: Option<ManualMove>,
    pending_layout: Option<LayoutRequest>,
    relocate_collapsed: bool,
    resize_generation: u64,
    layout_generation: u64,
    program_sizes: VecDeque<PhysicalSize<u32>>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            collapsed: None,
            expanded: false,
            menu: None,
            pending_restore: None,
            config: WindowConfig::default(),
            writable: true,
            natural_height: 240.0,
            min_height: 100.0,
            resize: None,
            movement: None,
            pending_layout: None,
            relocate_collapsed: false,
            resize_generation: 0,
            layout_generation: 0,
            program_sizes: VecDeque::new(),
        }
    }
}
impl Layout {
    fn active(&self) -> bool {
        self.resize.is_some() || self.movement.is_some()
    }

    fn adopt_system_gesture(&mut self, control: &native::Control) {
        let Some((generation, gesture)) = control.take_system_gesture() else {
            return;
        };
        self.resize_generation = generation;
        self.layout_generation = self.layout_generation.wrapping_add(1);
        if let Some(direction) = gesture.direction {
            let (horizontal, vertical) = if self.menu.is_some() {
                (false, false)
            } else {
                resize_axes(direction).unwrap_or((false, false))
            };
            self.resize = Some(ManualResize {
                generation,
                horizontal,
                vertical,
                start: gesture.size,
                changed_width: false,
                changed_height: false,
            });
        } else {
            self.movement = Some(ManualMove {
                generation,
                start: gesture.rect,
                scale: gesture.scale,
                origin: None,
            });
        }
    }

    fn repair_anchors(&mut self, available: &[Screen], current: Rect) -> bool {
        if available.is_empty() {
            return false;
        }
        let mut repaired = false;
        for rect in [
            &mut self.collapsed,
            &mut self.menu,
            &mut self.pending_restore,
        ]
        .into_iter()
        .flatten()
        {
            if available
                .iter()
                .all(|screen| rect.intersection(screen.work) == 0)
            {
                rect.x = current.x;
                rect.y = current.y;
                repaired = true;
            }
        }
        repaired
    }

    fn record_move(&mut self, current: Rect, scale: f64) {
        let Some(movement) = self.movement else {
            return;
        };
        if (current.x, current.y) == (movement.start.x, movement.start.y) {
            return;
        }
        self.relocate_collapsed = true;
        if let Some(compact) = self.collapsed.as_mut() {
            compact.x = current.x;
            compact.y = current.y;
            compact.width = (f64::from(compact.width) / movement.scale * scale)
                .round()
                .max(1.0) as u32;
            compact.height = (f64::from(compact.height) / movement.scale * scale)
                .round()
                .max(1.0) as u32;
        }
    }
    fn plan(&self, current: Rect, request: LayoutRequest) -> Option<(Self, Option<Rect>)> {
        if request
            .expected_generation
            .is_some_and(|generation| generation != self.layout_generation)
        {
            return None;
        }
        let mut next = self.clone();
        next.natural_height = request.height;
        next.min_height = request.min_height;
        let restore = if next.active() {
            next.pending_layout = Some(LayoutRequest {
                expected_generation: None,
                ..request
            });
            None
        } else {
            next.transition(current, request.expanded, request.menu_open)
        };
        next.layout_generation = next.layout_generation.wrapping_add(1);
        Some((next, restore))
    }

    fn dpi_request(&self) -> LayoutRequest {
        if let Some(request) = self.pending_layout {
            return LayoutRequest {
                expected_generation: Some(self.layout_generation),
                ..request
            };
        }
        LayoutRequest {
            height: self.natural_height,
            min_height: self.min_height,
            expanded: self.expanded,
            menu_open: self.menu.is_some(),
            expected_generation: Some(self.layout_generation),
        }
    }

    fn transition(&mut self, current: Rect, expanded: bool, menu_open: bool) -> Option<Rect> {
        let pending = self.pending_restore.take();
        let base = self.menu.or(pending).unwrap_or(current);
        let changed = expanded != self.expanded;
        let mut restore = pending;
        if changed {
            if expanded {
                self.collapsed = Some(base);
                restore = Some(base);
            } else {
                restore = self.collapsed.take().or(Some(base));
            }
        } else if !menu_open {
            restore = self.menu.or(restore);
        }
        self.menu = if menu_open {
            if changed {
                restore.or(Some(base))
            } else {
                self.menu.or(Some(base))
            }
        } else {
            None
        };
        self.expanded = expanded;
        restore
    }

    fn saved_rect(&self, current: Option<Rect>) -> Option<Rect> {
        self.collapsed
            .or(self.menu)
            .or(self.pending_restore)
            .or(current)
    }

    fn remember_program_size(&mut self, size: PhysicalSize<u32>) {
        if self.program_sizes.back() != Some(&size) {
            self.program_sizes.push_back(size);
            if self.program_sizes.len() > 16 {
                self.program_sizes.pop_front();
            }
        }
    }

    fn record_manual_size(&mut self, size: PhysicalSize<u32>, scale: f64) {
        let Some(resize) = self.resize.as_mut() else {
            return;
        };
        resize.changed_width |= resize.horizontal && size.width != resize.start.width;
        resize.changed_height |= resize.vertical && size.height != resize.start.height;
        let mode = self.config.mode_mut(self.expanded);
        if resize.changed_width {
            mode.width = (f64::from(size.width) / scale).clamp(MIN_WIDTH, MAX_WIDTH);
        }
        if resize.changed_height {
            mode.height = Some(f64::from(size.height) / scale);
        }
        self.layout_generation = self.layout_generation.wrapping_add(1);
    }

    fn reset_sizes(&mut self) {
        self.config.compact = ModeSize::default();
        self.config.expanded = ModeSize::default();
        self.resize_generation = self.resize_generation.wrapping_add(1);
        self.layout_generation = self.layout_generation.wrapping_add(1);
        self.resize = None;
        self.movement = None;
        self.pending_layout = None;
        self.relocate_collapsed = false;
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min_width: f64,
    max_width: f64,
    min_height: f64,
    max_height: f64,
}
impl Bounds {
    fn new(min_height: f64, scale: f64, work: Option<Rect>) -> Self {
        let available_width = work
            .map(|r| f64::from(r.width) / scale)
            .unwrap_or(MAX_WIDTH)
            .max(1.0);
        let max_width = MAX_WIDTH.min(available_width);
        let max_height = work
            .map(|r| f64::from(r.height) / scale)
            .unwrap_or(1000.0)
            .max(1.0);
        Self {
            min_width: MIN_WIDTH.min(max_width),
            max_width,
            min_height: min_height.max(100.0).min(max_height),
            max_height,
        }
    }
    fn size(
        self,
        mode: ModeSize,
        natural_height: f64,
        menu_open: bool,
        scale: f64,
    ) -> PhysicalSize<u32> {
        let height = mode.height.unwrap_or(natural_height);
        let height = if menu_open {
            height.max(natural_height)
        } else {
            height
        };
        PhysicalSize::new(
            (mode.width.clamp(self.min_width, self.max_width) * scale)
                .round()
                .max(1.0) as u32,
            (height.clamp(self.min_height, self.max_height) * scale)
                .round()
                .max(1.0) as u32,
        )
    }
    fn constraints(self, scale: f64) -> WindowSizeConstraints {
        let max_width = (self.max_width * scale).floor().max(1.0) as u32;
        let max_height = (self.max_height * scale).floor().max(1.0) as u32;
        WindowSizeConstraints {
            min_width: Some(
                PhysicalUnit(((self.min_width * scale).ceil() as u32).min(max_width)).into(),
            ),
            min_height: Some(
                PhysicalUnit(((self.min_height * scale).ceil() as u32).min(max_height)).into(),
            ),
            max_width: Some(PhysicalUnit(max_width).into()),
            max_height: Some(PhysicalUnit(max_height).into()),
        }
    }
}

fn resize_axes(direction: &str) -> Option<(bool, bool)> {
    match direction {
        "East" | "West" => Some((true, false)),
        "North" | "South" => Some((false, true)),
        "NorthEast" | "NorthWest" | "SouthEast" | "SouthWest" => Some((true, true)),
        _ => None,
    }
}
pub struct Windows {
    layout: Mutex<Layout>,
    saving: Mutex<Option<Vec<u8>>>,
    details: Mutex<()>,
    path: PathBuf,
    native: Arc<native::Control>,
}
impl Windows {
    pub fn new(path: PathBuf) -> Self {
        Self {
            layout: Mutex::new(Layout::default()),
            saving: Mutex::new(None),
            details: Mutex::new(()),
            path,
            native: Arc::new(native::Control::default()),
        }
    }
    pub fn install_native_hook(&self, app: &AppHandle) -> Result<(), String> {
        native::install(app, self.native.clone())
    }
    pub fn restore(&self, app: &AppHandle) {
        let Some(window) = app.get_webview_window("float") else {
            return;
        };
        if let Ok(bytes) = std::fs::read(&self.path) {
            if let Ok(mut layout) = self.layout.lock() {
                match WindowConfig::decode(&bytes) {
                    Ok(config) => {
                        layout.config = config;
                        if let Some(position) = layout.config.position {
                            let rect = Rect {
                                x: position.x,
                                y: position.y,
                                width: 316,
                                height: 240,
                            };
                            layout.pending_restore = Some(rect);
                            let _ = window.set_position(PhysicalPosition::new(rect.x, rect.y));
                        }
                    }
                    Err(()) => layout.writable = false,
                }
            }
        } else if let Ok(Some(monitor)) = window.primary_monitor() {
            let area = monitor.work_area();
            let _ = window.set_position(PhysicalPosition::new(
                area.position.x + area.size.width as i32 - 340,
                area.position.y + 80,
            ));
        }
        self.clamp(app);
    }
    pub fn save(&self, app: &AppHandle) {
        let _ = self.save_checked(app);
    }
    pub fn save_checked(&self, app: &AppHandle) -> Result<(), String> {
        // Query the UI before taking the save lock; UI callbacks may save concurrently.
        // The save lock must never make the event loop wait for a thread querying that loop.
        let current = capture(app);
        let mut saving = self.saving.lock().map_err(|_| "窗口保存不可用")?;
        let mut layout = self
            .layout
            .try_lock()
            .map_err(|_| "窗口正在调整，请稍后重试")?;
        if !layout.writable {
            return Err("窗口配置无法保存，原文件已保留。".into());
        }
        if layout.active() || self.native.active() {
            return Err("请结束浮窗拖动或缩放后再安装更新。".into());
        }
        if let Some(rect) = layout.saved_rect(current) {
            layout.config.position = Some(SavedPosition {
                x: rect.x,
                y: rect.y,
            });
        }
        let config = layout.config.clone();
        drop(layout);
        let bytes = serde_json::to_vec(&config).map_err(|_| "窗口配置编码失败")?;
        if saving.as_ref() != Some(&bytes) {
            crate::core::atomic_write(&self.path, &bytes)?;
            *saving = Some(bytes);
        }
        Ok(())
    }
    pub fn clamp(&self, app: &AppHandle) {
        if self.native.active() {
            return;
        }
        let Some(window) = app.get_webview_window("float") else {
            return;
        };
        let Ok(pos) = window.outer_position() else {
            return;
        };
        let Ok(size) = window.outer_size() else {
            return;
        };
        let current = Rect {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        };
        if let Some(screen) = select_screen(&screens(&window), current, None, None) {
            let rect = Rect {
                width: current.width.min(screen.work.width),
                height: current.height.min(screen.work.height),
                ..current
            }
            .clamped(screen.work);
            if (size.width, size.height) != (rect.width, rect.height) {
                let _ = window.set_size(PhysicalSize::new(rect.width, rect.height));
            }
            if pos.x != rect.x || pos.y != rect.y {
                let _ = window.set_position(PhysicalPosition::new(rect.x, rect.y));
            }
        }
    }
    pub fn on_moved(&self, app: &AppHandle) {
        // Programmatic layout restores a position before changing the height.
        // Do not clamp that intermediate rectangle with the previous size.
        let Ok(mut layout) = self.layout.try_lock() else {
            return;
        };
        layout.adopt_system_gesture(&self.native);
        if layout.active() || self.native.active() {
            return;
        }
        self.clamp(app);
        drop(layout);
        self.save(app);
    }
    pub fn layout(
        &self,
        app: &AppHandle,
        height: f64,
        min_height: Option<f64>,
        expanded: bool,
        menu_open: bool,
    ) -> Result<FloatSizeState, String> {
        self.layout_request(
            app,
            LayoutRequest {
                height,
                min_height: min_height.unwrap_or(height),
                expanded,
                menu_open,
                expected_generation: None,
            },
        )
    }

    fn layout_request(
        &self,
        app: &AppHandle,
        request: LayoutRequest,
    ) -> Result<FloatSizeState, String> {
        self.layout_request_on_screen(app, request, None)
    }

    fn layout_request_on_screen(
        &self,
        app: &AppHandle,
        request: LayoutRequest,
        destination: Option<Screen>,
    ) -> Result<FloatSizeState, String> {
        let LayoutRequest {
            height,
            min_height,
            menu_open,
            ..
        } = request;
        if !height.is_finite() || height < 1.0 || !min_height.is_finite() || min_height < 1.0 {
            return Err("浮窗尺寸无效".into());
        }
        let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
        let current = capture(app).ok_or("无法读取浮窗位置")?;
        let mut state = self.layout.lock().map_err(|_| "窗口状态不可用")?;
        state.adopt_system_gesture(&self.native);
        // The generation is checked while holding the same mutex used to commit layout.
        // An old DPI task cannot reapply a mode/menu snapshot after a newer user action.
        let Some((mut next, restore)) = state.plan(current, request) else {
            return self.size_state(&window, &state);
        };
        if next.active() || self.native.active() {
            *state = next;
            return self.size_state(&window, &state);
        }
        if let Some(rect) = restore {
            window
                .set_position(PhysicalPosition::new(rect.x, rect.y))
                .map_err(|_| "无法恢复浮窗位置")?;
        }
        // Choose from individual monitors, including negative coordinates and layout gaps.
        let current = capture(app).ok_or("无法读取浮窗位置")?;
        let monitor = destination.or_else(|| select_screen(&screens(&window), current, None, None));
        let scale = match monitor {
            Some(monitor) => monitor.scale,
            None => window.scale_factor().map_err(|_| "无法读取显示缩放")?,
        };
        let area = monitor.map(|m| m.work);
        let bounds = Bounds::new(next.min_height, scale, area);
        let mode = next.config.mode(next.expanded);
        let size = bounds.size(mode, height, menu_open, scale);
        next.remember_program_size(size);
        window
            .set_size_constraints(bounds.constraints(scale))
            .map_err(|_| "无法设置浮窗大小范围")?;
        if window.inner_size().ok() != Some(size) {
            window.set_size(size).map_err(|_| "无法调整浮窗大小")?;
        }
        if let Some(area) = area {
            let actual = capture(app).ok_or("无法读取浮窗位置")?;
            let final_rect = actual.clamped(area);
            if (actual.x, actual.y) != (final_rect.x, final_rect.y) {
                window
                    .set_position(PhysicalPosition::new(final_rect.x, final_rect.y))
                    .map_err(|_| "无法恢复浮窗位置")?;
            }
        }
        if next.relocate_collapsed {
            if let (Some(compact), Some(actual)) = (next.collapsed.as_mut(), capture(app)) {
                compact.x = actual.x;
                compact.y = actual.y;
            }
            next.relocate_collapsed = false;
        }
        *state = next;
        let result = self.size_state(&window, &state)?;
        drop(state);
        // Moved callbacks during resize must not persist an intermediate rectangle.
        self.save(app);
        self.emit_size(app, result);
        Ok(result)
    }

    fn size_state(&self, window: &WebviewWindow, state: &Layout) -> Result<FloatSizeState, String> {
        let size = window.inner_size().map_err(|_| "无法读取浮窗大小")?;
        let scale = window.scale_factor().map_err(|_| "无法读取显示缩放")?;
        Ok(FloatSizeState {
            width: f64::from(size.width) / scale,
            height: f64::from(size.height) / scale,
            manual_height: state.config.mode(state.expanded).height.is_some(),
            resizing: state.resize.is_some(),
        })
    }

    fn emit_size(&self, app: &AppHandle, value: FloatSizeState) {
        if let Some(window) = app.get_webview_window("float") {
            let _ = window.emit("subgauge:float-size", value);
        }
    }

    pub fn on_resized(&self, app: &AppHandle, size: PhysicalSize<u32>) {
        let Some(window) = app.get_webview_window("float") else {
            return;
        };
        let Ok(mut state) = self.layout.try_lock() else {
            return;
        };
        state.adopt_system_gesture(&self.native);
        if state.movement.is_some() {
            return;
        }
        // Resize notifications can arrive after a newer layout. Never publish/save the old size.
        if window.inner_size().ok() != Some(size) || state.program_sizes.contains(&size) {
            return;
        }
        if let Ok(scale) = window.scale_factor() {
            state.record_manual_size(size, scale);
        }
        let result = self.size_state(&window, &state).ok();
        drop(state);
        if let Some(result) = result {
            self.emit_size(app, result);
        }
    }

    pub fn on_scale_changed(&self, app: &AppHandle) {
        let Ok(state) = self.layout.try_lock() else {
            return;
        };
        let request = state.dpi_request();
        drop(state);
        native::schedule(app.clone(), move |app| {
            let _ = app.state::<Windows>().layout_request(&app, request);
        });
    }

    fn begin_resize(&self, app: &AppHandle, direction: &str) -> Result<u64, String> {
        let (horizontal, vertical) = resize_axes(direction).ok_or("调整方向无效")?;
        let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
        let start = window.inner_size().map_err(|_| "无法读取浮窗大小")?;
        let mut state = self.layout.lock().map_err(|_| "窗口状态不可用")?;
        if state.menu.is_some() {
            return Err("请先关闭账号菜单再调整大小".into());
        }
        if state.active() || self.native.active() {
            return Err("正在移动或调整浮窗大小".into());
        }
        state.resize_generation = self.native.next_generation();
        state.layout_generation = state.layout_generation.wrapping_add(1);
        let generation = state.resize_generation;
        state.resize = Some(ManualResize {
            generation,
            horizontal,
            vertical,
            start,
            changed_width: false,
            changed_height: false,
        });
        self.native.begin(generation);
        let result = self.size_state(&window, &state)?;
        drop(state);
        self.emit_size(app, result);
        Ok(generation)
    }

    fn begin_move(&self, app: &AppHandle) -> Result<u64, String> {
        let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
        let start = capture(app).ok_or("无法读取浮窗位置")?;
        let scale = window.scale_factor().map_err(|_| "无法读取显示缩放")?;
        let origin = select_screen(&screens(&window), start, None, None).map(|s| s.bounds);
        let mut state = self.layout.lock().map_err(|_| "窗口状态不可用")?;
        if state.menu.is_some() {
            return Err("请先关闭账号菜单再移动浮窗".into());
        }
        if state.active() || self.native.active() {
            return Err("正在移动或调整浮窗大小".into());
        }
        state.resize_generation = self.native.next_generation();
        state.layout_generation = state.layout_generation.wrapping_add(1);
        let generation = state.resize_generation;
        state.movement = Some(ManualMove {
            generation,
            start,
            scale,
            origin,
        });
        self.native.begin(generation);
        Ok(generation)
    }

    fn finish_gesture(&self, app: &AppHandle, generation: u64) {
        let Some(window) = app.get_webview_window("float") else {
            return;
        };
        let Ok(mut state) = self.layout.lock() else {
            return;
        };
        state.adopt_system_gesture(&self.native);
        if state
            .resize
            .map(|r| r.generation)
            .or(state.movement.map(|m| m.generation))
            != Some(generation)
        {
            return;
        }
        let current = capture(app);
        let destination = current.and_then(|rect| {
            select_screen(
                &screens(&window),
                rect,
                native::pointer(),
                state.movement.and_then(|m| m.origin),
            )
        });
        let scale = destination
            .map(|s| s.scale)
            .or_else(|| window.scale_factor().ok())
            .unwrap_or(1.0);
        if let Ok(size) = window.inner_size() {
            state.record_manual_size(size, scale);
        }
        if let Some(current) = current {
            state.record_move(current, scale);
        }
        state.resize = None;
        state.movement = None;
        state.layout_generation = state.layout_generation.wrapping_add(1);
        self.native.finish(generation);
        let request = state
            .pending_layout
            .take()
            .unwrap_or_else(|| state.dpi_request());
        drop(state);
        let _ = self.layout_request_on_screen(app, request, destination);
    }

    pub fn recover(&self, app: &AppHandle) {
        if self.native.active() {
            return;
        }
        let Ok(state) = self.layout.try_lock() else {
            return;
        };
        let request = state.dpi_request();
        drop(state);
        let _ = self.layout_request(app, request);
        if let (Some(window), Some(current)) = (app.get_webview_window("float"), capture(app)) {
            if let Ok(mut state) = self.layout.try_lock() {
                state.repair_anchors(&screens(&window), current);
            }
            self.save(app);
        }
    }

    fn reset(&self, app: &AppHandle) -> Result<(), String> {
        let mut state = self.layout.lock().map_err(|_| "窗口状态不可用")?;
        if state.active() || self.native.active() {
            return Err("请在拖动结束后恢复默认大小".into());
        }
        state.reset_sizes();
        let request = state.dpi_request();
        drop(state);
        self.layout_request(app, request)?;
        Ok(())
    }
}
fn capture(app: &AppHandle) -> Option<Rect> {
    let window = app.get_webview_window("float")?;
    let p = window.outer_position().ok()?;
    let s = window.outer_size().ok()?;
    Some(Rect {
        x: p.x,
        y: p.y,
        width: s.width,
        height: s.height,
    })
}
pub fn show_float_impl(app: &AppHandle) -> Result<(), String> {
    let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
    window.unminimize().map_err(|_| "无法恢复浮窗")?;
    app.state::<Windows>().recover(app);
    window.show().map_err(|_| "无法显示浮窗")?;
    window.set_focus().map_err(|_| "无法激活浮窗".into())
}
pub fn open_details_impl(
    app: &AppHandle,
    page: Option<String>,
    account_id: Option<String>,
) -> Result<(), String> {
    let windows = app.state::<Windows>();
    let _creation = windows.details.lock().map_err(|_| "详情窗口状态不可用")?;
    let page = page
        .filter(|p| {
            [
                "overview",
                "records",
                "analysis",
                "accounts",
                "settings",
                "login",
                "preferences",
            ]
            .contains(&p.as_str())
        })
        .unwrap_or_else(|| "overview".into());
    if let Some(window) = app.get_webview_window("details") {
        window
            .emit(
                "subgauge:navigate",
                serde_json::json!({"page":page,"accountId":account_id}),
            )
            .map_err(|_| "无法切换页面")?;
        let _ = window.unminimize();
        window.show().map_err(|_| "无法打开详情")?;
        return window.set_focus().map_err(|_| "无法激活详情".into());
    }
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query
        .append_pair("window", "details")
        .append_pair("page", &page);
    if let Some(id) = account_id {
        query.append_pair("accountId", &id);
    }
    WebviewWindowBuilder::new(
        app,
        "details",
        WebviewUrl::App(format!("index.html?{}", query.finish()).into()),
    )
    .title("SubGauge · 用量详情")
    .inner_size(940.0, 720.0)
    .min_inner_size(660.0, 520.0)
    .center()
    .build()
    .map_err(|_| "无法创建详情窗口")?;
    Ok(())
}
#[tauri::command]
pub async fn open_details(
    app: AppHandle,
    page: Option<String>,
    account_id: Option<String>,
) -> Result<(), String> {
    open_details_impl(&app, page, account_id)
}
#[tauri::command]
pub fn show_float(app: AppHandle) -> Result<(), String> {
    show_float_impl(&app)
}
#[tauri::command]
pub async fn set_float_layout(
    app: AppHandle,
    height: f64,
    min_height: Option<f64>,
    expanded: bool,
    menu_open: Option<bool>,
) -> Result<FloatSizeState, String> {
    on_ui(app, move |app| {
        check_update_install(&app)?;
        app.state::<Windows>().layout(
            &app,
            height,
            min_height,
            expanded,
            menu_open.unwrap_or(false),
        )
    })
    .await
}

pub(crate) async fn on_ui<T: Send + 'static>(
    app: AppHandle,
    task: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let target = app.clone();
    app.run_on_main_thread(move || {
        let _ = send.send(task(target));
    })
    .map_err(|_| "窗口线程不可用")?;
    receive.await.map_err(|_| "窗口操作已取消")?
}

fn check_gesture_start(app: AppHandle, generation: u64) {
    // One bounded recovery for a dispatched gesture that never entered the
    // system loop (e.g. pointer released before dispatch). No idle polling.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let _ = on_ui(app, move |app| {
            if app.state::<Windows>().native.pending(generation) {
                app.state::<Windows>().finish_gesture(&app, generation);
            }
            Ok(())
        })
        .await;
    });
}

#[tauri::command]
pub async fn start_float_drag(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.label() != "float" {
        return Err("只能移动浮窗".into());
    }
    on_ui(app, move |app| {
        check_update_install(&app)?;
        if !app.state::<Windows>().native.ready() {
            return Err("浮窗移动监听不可用".into());
        }
        let generation = app.state::<Windows>().begin_move(&app)?;
        if window.start_dragging().is_err() {
            app.state::<Windows>().finish_gesture(&app, generation);
            return Err("无法开始移动浮窗".into());
        }
        check_gesture_start(app, generation);
        Ok(())
    })
    .await
}
#[tauri::command]
pub async fn start_float_resize(
    app: AppHandle,
    window: WebviewWindow,
    direction: String,
) -> Result<(), String> {
    if window.label() != "float" {
        return Err("只能调整浮窗大小".into());
    }
    on_ui(app, move |app| {
        check_update_install(&app)?;
        if !app.state::<Windows>().native.ready() {
            return Err("浮窗移动监听不可用".into());
        }
        let generation = app.state::<Windows>().begin_resize(&app, &direction)?;
        let native_direction = serde_json::from_value(serde_json::Value::String(direction))
            .map_err(|_| "调整方向无效")?;
        let webview: &tauri::Webview = window.as_ref();
        if webview
            .window()
            .start_resize_dragging(native_direction)
            .is_err()
        {
            app.state::<Windows>().finish_gesture(&app, generation);
            return Err("无法开始调整浮窗大小".into());
        }
        check_gesture_start(app, generation);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn reset_float_size(app: AppHandle) -> Result<(), String> {
    on_ui(app, move |app| {
        check_update_install(&app)?;
        app.state::<Windows>().reset(&app)
    })
    .await
}
fn check_update_install(app: &AppHandle) -> Result<(), String> {
    if app
        .try_state::<crate::updates::Updates>()
        .is_some_and(|updates| updates.is_installing())
    {
        return Err("正在准备安装更新，请稍后调整浮窗。".into());
    }
    Ok(())
}
pub async fn actual_topmost(app: AppHandle) -> Result<bool, String> {
    on_ui(app, |app| {
        let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
        native::topmost(&window)
    })
    .await
}
pub async fn apply_topmost(app: AppHandle, value: bool) -> Result<(), String> {
    on_ui(app, move |app| {
        set_always_on_top(app.clone(), value)?;
        let window = app.get_webview_window("float").ok_or("浮窗不可用")?;
        if native::topmost(&window)? != value {
            return Err("浮窗置顶状态未能更新".into());
        }
        Ok(())
    })
    .await
}
pub fn set_always_on_top(app: AppHandle, value: bool) -> Result<(), String> {
    app.get_webview_window("float")
        .ok_or("浮窗不可用")?
        .set_always_on_top(value)
        .map_err(|_| "无法设置置顶".into())
}

pub fn handle_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "float" {
        return;
    }
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = window.hide();
        }
        tauri::WindowEvent::Moved(_) => window
            .app_handle()
            .state::<Windows>()
            .on_moved(window.app_handle()),
        tauri::WindowEvent::Resized(size) => window
            .app_handle()
            .state::<Windows>()
            .on_resized(window.app_handle(), *size),
        tauri::WindowEvent::ScaleFactorChanged { .. } => window
            .app_handle()
            .state::<Windows>()
            .on_scale_changed(window.app_handle()),
        _ => {}
    }
}
