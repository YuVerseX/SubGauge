use super::*;

fn card(y: i32, height: u32) -> Rect {
    Rect {
        x: 120,
        y,
        width: 316,
        height,
    }
}

#[test]
fn closing_menu_and_expanding_preserves_original_card_position() {
    let compact = card(800, 200);
    let menu = card(650, 350);
    let expanded = card(600, 400);
    let mut layout = Layout::default();
    layout.transition(compact, false, true);
    assert_eq!(layout.transition(menu, true, false), Some(compact));
    assert_eq!(layout.saved_rect(Some(expanded)), Some(compact));
    assert_eq!(layout.transition(expanded, false, false), Some(compact));
}

#[test]
fn collapsing_with_menu_open_does_not_restore_expanded_position_later() {
    let compact = card(800, 200);
    let expanded = card(600, 400);
    let menu = card(500, 500);
    let mut layout = Layout::default();
    layout.transition(compact, true, false);
    layout.transition(expanded, true, true);
    assert_eq!(layout.transition(menu, false, true), Some(compact));
    assert_eq!(layout.saved_rect(Some(menu)), Some(compact));
    assert_eq!(layout.transition(menu, false, false), Some(compact));
}

#[test]
fn expanding_with_menu_open_keeps_the_compact_baseline() {
    let compact = card(800, 200);
    let menu = card(650, 350);
    let expanded = card(600, 400);
    let mut layout = Layout::default();
    layout.transition(compact, false, true);
    layout.transition(menu, true, true);
    layout.transition(expanded, true, false);
    assert_eq!(layout.transition(expanded, false, false), Some(compact));
}

#[test]
fn startup_uses_saved_compact_position_after_initial_height_clamping() {
    let saved = card(850, 150);
    let initial = card(760, 240);
    let mut layout = Layout {
        pending_restore: Some(saved),
        ..Default::default()
    };
    assert_eq!(layout.saved_rect(Some(initial)), Some(saved));
    assert_eq!(layout.transition(initial, false, false), Some(saved));
    assert!(layout.pending_restore.is_none());
    assert_eq!(layout.transition(saved, false, false), None);
}

#[test]
fn temporary_menu_position_is_never_saved() {
    let compact = card(800, 200);
    let menu = card(650, 350);
    let mut layout = Layout::default();
    layout.transition(compact, false, true);
    assert_eq!(layout.saved_rect(Some(menu)), Some(compact));
    assert_eq!(layout.transition(menu, false, false), Some(compact));
    assert_eq!(layout.saved_rect(Some(compact)), Some(compact));
}

#[test]
fn dragging_an_expanded_card_reanchors_the_compact_card() {
    let compact = card(800, 200);
    let dragged = Rect {
        x: 400,
        y: 300,
        ..card(600, 400)
    };
    let mut layout = Layout::default();
    layout.transition(compact, true, false);
    layout.movement = Some(ManualMove {
        generation: 1,
        start: card(600, 400),
        scale: 1.0,
        origin: None,
    });
    layout.record_move(dragged, 1.5);
    layout.movement = None;
    let moved_compact = Rect {
        x: 400,
        y: 300,
        width: 474,
        height: 300,
    };
    assert_eq!(layout.saved_rect(Some(dragged)), Some(moved_compact));
    assert_eq!(
        layout.transition(dragged, false, false),
        Some(moved_compact)
    );
    assert_eq!(layout.saved_rect(Some(dragged)), Some(dragged));
}

fn screen(x: i32, y: i32, width: u32, height: u32, scale: f64) -> Screen {
    Screen {
        bounds: Rect {
            x,
            y,
            width,
            height,
        },
        work: Rect {
            x,
            y,
            width,
            height: height - 40,
        },
        scale,
    }
}

#[test]
fn slow_drag_can_cross_a_shared_monitor_edge_before_final_clamping() {
    let monitors = [
        screen(0, 0, 1920, 1080, 1.0),
        screen(1920, 0, 1920, 1080, 1.0),
    ];
    let mut layout = Layout::default();
    let mut current = Rect {
        x: 1604,
        y: 100,
        width: 316,
        height: 240,
    };
    layout.movement = Some(ManualMove {
        generation: 1,
        start: current,
        scale: 1.0,
        origin: Some(monitors[0].bounds),
    });
    for _ in 0..50 {
        current.x += 8;
        let (next, restore) = layout.plan(current, layout.dpi_request()).unwrap();
        assert!(restore.is_none());
        assert!(next.active());
        layout = next;
    }
    let target = select_screen(&monitors, current, None, Some(monitors[0].bounds)).unwrap();
    assert_eq!(target.bounds.x, 1920);
    assert_eq!(current.clamped(target.work).x, 2004);
}

#[test]
fn monitor_ties_use_pointer_then_origin_and_stable_coordinates() {
    let monitors = [
        screen(0, 0, 1920, 1080, 1.0),
        screen(1920, 0, 1920, 1080, 1.5),
    ];
    let seam = Rect {
        x: 1762,
        y: 100,
        width: 316,
        height: 240,
    };
    assert_eq!(
        select_screen(&monitors, seam, Some((1920, 200)), Some(monitors[0].bounds))
            .unwrap()
            .scale,
        1.5
    );
    assert_eq!(
        select_screen(&monitors, seam, None, Some(monitors[1].bounds))
            .unwrap()
            .scale,
        1.5
    );
    assert_eq!(
        select_screen(&monitors, seam, None, None).unwrap().scale,
        1.0
    );
    assert_eq!(
        select_screen(&[monitors[1], monitors[0]], seam, None, None)
            .unwrap()
            .scale,
        1.0
    );
}

#[test]
fn negative_coordinates_layout_gaps_and_disconnected_screens_use_real_monitors() {
    let monitors = [
        screen(0, 0, 1920, 1080, 1.0),
        screen(-1080, -1920, 1080, 1920, 1.5),
    ];
    let gap = Rect {
        x: 300,
        y: -500,
        width: 316,
        height: 240,
    };
    let target = select_screen(&monitors, gap, None, None).unwrap();
    assert_eq!(target.bounds.x, 0);
    assert_eq!(gap.clamped(target.work).y, 0);
    let removed = Rect {
        x: -900,
        y: -1500,
        ..gap
    };
    assert_eq!(
        select_screen(&monitors, removed, None, None)
            .unwrap()
            .bounds
            .x,
        -1080
    );
    let remaining = &monitors[..1];
    assert_eq!(
        removed
            .clamped(select_screen(remaining, removed, None, None).unwrap().work)
            .x,
        0
    );
    assert!(select_screen(&[], removed, None, None).is_none());
}

#[test]
fn zero_movement_and_returning_to_the_start_keep_the_original_compact_anchor() {
    let compact = card(800, 200);
    let expanded = card(600, 400);
    let mut layout = Layout::default();
    layout.transition(compact, true, false);
    layout.movement = Some(ManualMove {
        generation: 1,
        start: expanded,
        scale: 1.0,
        origin: None,
    });
    layout.record_move(expanded, 2.0);
    assert_eq!(layout.collapsed, Some(compact));
    assert!(!layout.relocate_collapsed);
}

#[test]
fn moving_defers_the_latest_layout_and_dpi_does_not_restore_an_older_mode() {
    let mut layout = Layout {
        movement: Some(ManualMove {
            generation: 1,
            start: card(100, 240),
            scale: 1.0,
            origin: None,
        }),
        ..Default::default()
    };
    let stale = layout.dpi_request();
    let (next, restore) = layout
        .plan(
            card(100, 240),
            LayoutRequest {
                height: 600.0,
                min_height: 350.0,
                expanded: true,
                menu_open: false,
                expected_generation: None,
            },
        )
        .unwrap();
    layout = next;
    assert!(restore.is_none());
    assert!(!layout.expanded);
    assert!(layout.dpi_request().expanded);
    assert_eq!(layout.pending_layout.unwrap().height, 600.0);
    assert!(layout.plan(card(100, 240), stale).is_none());
    assert_eq!(layout.config.compact, ModeSize::default());
}

#[test]
fn native_lifecycle_ends_once_and_old_completion_cannot_end_the_next_gesture() {
    let control = native::Control::default();
    control.begin(1);
    assert!(control.pending(1));
    control.entered();
    assert!(!control.pending(1));
    assert_eq!(control.request_end(), Some(1));
    assert_eq!(control.request_end(), None);
    control.finish(1);
    assert!(!control.active());
    control.begin(2);
    control.finish(1);
    assert!(control.active());
    assert_eq!(control.request_end(), Some(2));
    control.finish(2);
    assert!(!control.active());
}

#[cfg(windows)]
#[test]
#[ignore = "requires a Windows desktop with WebView2; runs an isolated hidden window"]
fn native_window_smoke() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageW, HTRIGHT, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE,
    };
    let directory =
        std::env::temp_dir().join(format!("subgauge-window-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let result = Arc::new(Mutex::new(None::<String>));
    let control = Arc::new(Mutex::new(None::<Arc<native::Control>>));
    let control_setup = control.clone();
    let result_setup = result.clone();
    let test_directory = directory.clone();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier =
        format!("app.subgauge.test.{}", uuid::Uuid::new_v4().simple());
    let application = tauri::Builder::default()
        .any_thread()
        .on_window_event(handle_event)
        .setup(move |app| {
            app.manage(Windows::new(test_directory.join("window.json")));
            let window = WebviewWindowBuilder::new(
                app,
                "float",
                WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .visible(false)
            .inner_size(316.0, 240.0)
            .data_directory(test_directory.join("webview"))
            .build()?;
            let service = app.state::<Windows>();
            service
                .install_native_hook(app.handle())
                .map_err(std::io::Error::other)?;
            *control_setup.lock().unwrap() = Some(service.native.clone());
            service
                .layout(app.handle(), 240.0, Some(100.0), false, false)
                .map_err(std::io::Error::other)?;
            service
                .layout(app.handle(), 480.0, Some(300.0), true, false)
                .map_err(std::io::Error::other)?;
            set_always_on_top(app.handle().clone(), true).map_err(std::io::Error::other)?;
            assert!(native::topmost(&window).unwrap());
            set_always_on_top(app.handle().clone(), false).map_err(std::io::Error::other)?;
            assert!(!native::topmost(&window).unwrap());
            let generation = service
                .begin_move(app.handle())
                .map_err(std::io::Error::other)?;
            let hwnd = window.hwnd()?.0;
            unsafe {
                SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0);
            }
            assert!(!service.native.pending(generation));
            let start = capture(app.handle()).unwrap();
            let work = select_screen(&screens(&window), start, None, None)
                .unwrap()
                .work;
            let moved = Rect {
                x: work.x + 50,
                y: work.y + 50,
                ..start
            };
            window.set_position(PhysicalPosition::new(moved.x, moved.y))?;
            // A layout notification during movement must not change actual dimensions.
            let during = service
                .layout(app.handle(), 500.0, Some(300.0), true, false)
                .map_err(std::io::Error::other)?;
            assert_eq!(
                during.height,
                f64::from(start.height) / window.scale_factor()?
            );
            unsafe {
                SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut resized = false;
                for _ in 0..100 {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    let handle_task = handle.clone();
                    let result_task = result_setup.clone();
                    let was_resized = resized;
                    let done = on_ui(handle_task, move |app| {
                        let service = app.state::<Windows>();
                        if service.native.active() {
                            return Ok((false, was_resized));
                        }
                        let window = app
                            .get_webview_window("float")
                            .ok_or("missing float window")?;
                        if was_resized {
                            let state = service.layout.lock().unwrap();
                            if state.config.compact.width <= DEFAULT_WIDTH
                                || state.config.compact.height.is_some()
                            {
                                return Err(
                                    "system border resize was not persisted correctly".into()
                                );
                            }
                            drop(state);
                            *result_task.lock().unwrap() = Some("passed".into());
                            window
                                .destroy()
                                .map_err(|_| "cannot destroy isolated native window")?;
                            app.exit(0);
                            return Ok((true, true));
                        }
                        let before = capture(&app).ok_or("missing expanded rectangle")?;
                        service.layout(&app, 240.0, Some(100.0), false, false)?;
                        let compact = capture(&app).ok_or("missing compact rectangle")?;
                        let error = if (before.x, before.y) != (compact.x, compact.y) {
                            Some("collapse returned to an old position".into())
                        } else {
                            None
                        };
                        if let Some(error) = error {
                            return Err(error);
                        }
                        let hwnd = window.hwnd().map_err(|_| "missing HWND")?.0;
                        // The same capture used by Wry's outer-border WM_NCLBUTTONDOWN,
                        // without synthesizing mouse input or taking desktop capture.
                        native::capture_system_gesture(hwnd, HTRIGHT as usize, &service.native);
                        unsafe {
                            SendMessageW(hwnd, WM_ENTERSIZEMOVE, 0, 0);
                        }
                        let size = window.inner_size().map_err(|_| "missing size")?;
                        window
                            .set_size(PhysicalSize::new(size.width + 30, size.height))
                            .map_err(|_| "cannot resize test window")?;
                        unsafe {
                            SendMessageW(hwnd, WM_EXITSIZEMOVE, 0, 0);
                        }
                        Ok((false, true))
                    })
                    .await;
                    let (done, started) = match done {
                        Ok(done) => done,
                        Err(error) => {
                            *result_setup.lock().unwrap() = Some(error);
                            handle.exit(1);
                            return;
                        }
                    };
                    resized = started;
                    if done {
                        return;
                    }
                }
                *result_setup.lock().unwrap() = Some("native gesture did not finish".into());
                handle.exit(1);
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    let exit = application.run_return(|_, _| {});
    let outcome = result.lock().unwrap().clone();
    // The path is a unique, explicitly created test directory, never user data.
    let mut cleanup = std::fs::remove_dir_all(&directory);
    for _ in 0..10 {
        if cleanup.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        cleanup = std::fs::remove_dir_all(&directory);
    }
    assert!(
        cleanup.is_ok(),
        "cannot clean isolated native test directory"
    );
    assert_eq!(exit, 0);
    assert_eq!(outcome.as_deref(), Some("passed"));
    let control = control.lock().unwrap();
    assert!(
        !control.as_ref().unwrap().ready(),
        "native listener survived window destruction"
    );
    assert!(!control.as_ref().unwrap().active());
}

#[test]
fn recovery_repairs_only_anchors_that_belong_to_a_disconnected_screen() {
    let compact = Rect {
        x: -1500,
        y: 200,
        ..card(200, 240)
    };
    let target = card(100, 480);
    let mut layout = Layout {
        collapsed: Some(compact),
        menu: Some(compact),
        pending_restore: Some(compact),
        ..Default::default()
    };
    assert!(layout.repair_anchors(&[screen(0, 0, 1920, 1080, 1.0)], target));
    assert_eq!(layout.collapsed.unwrap().x, target.x);
    assert_eq!(layout.menu.unwrap().y, target.y);
    let repaired = layout.collapsed;
    assert!(!layout.repair_anchors(&[screen(0, 0, 1920, 1080, 1.0)], card(600, 480)));
    assert_eq!(layout.collapsed, repaired);
}

#[test]
fn a_changed_work_area_keeps_restored_windows_visible() {
    let area = Rect {
        x: -1920,
        y: 0,
        width: 1920,
        height: 1000,
    };
    let restored = Rect {
        x: 1850,
        y: 950,
        width: 632,
        height: 300,
    }
    .clamped(area);
    assert_eq!((restored.x, restored.y), (-632, 700));
}

#[test]
fn float_size_respects_destination_dpi_and_physical_work_area() {
    let area = Rect {
        width: 1920,
        height: 1000,
        ..Rect::default()
    };
    assert_eq!(
        Bounds::new(100.0, 1.0, Some(area)).size(ModeSize::default(), 150.0, false, 1.0),
        PhysicalSize::new(316, 150)
    );
    assert_eq!(
        Bounds::new(100.0, 2.0, Some(area)).size(ModeSize::default(), 150.0, false, 2.0),
        PhysicalSize::new(632, 300)
    );
    assert_eq!(
        Bounds::new(100.0, 2.0, Some(area)).size(ModeSize::default(), 800.0, false, 2.0),
        PhysicalSize::new(632, 1000)
    );
}

fn gesture(layout: &mut Layout, direction: &str, start: PhysicalSize<u32>) {
    let (horizontal, vertical) = resize_axes(direction).unwrap();
    layout.resize_generation += 1;
    layout.resize = Some(ManualResize {
        generation: layout.resize_generation,
        horizontal,
        vertical,
        start,
        changed_width: false,
        changed_height: false,
    });
}

#[test]
fn single_axis_drag_and_reload_preserve_independent_mode_preferences() {
    let mut layout = Layout::default();
    gesture(&mut layout, "East", PhysicalSize::new(474, 300));
    layout.record_manual_size(PhysicalSize::new(600, 330), 1.5);
    assert_eq!(
        layout.config.compact,
        ModeSize {
            width: 400.0,
            height: None
        }
    );
    layout.resize = None;
    layout.transition(card(800, 220), true, false);
    gesture(&mut layout, "South", PhysicalSize::new(474, 600));
    layout.record_manual_size(PhysicalSize::new(500, 750), 1.5);
    assert_eq!(
        layout.config.expanded,
        ModeSize {
            width: 316.0,
            height: Some(500.0)
        }
    );
    layout.resize = None;
    assert_eq!(
        layout.transition(card(600, 500), false, false),
        Some(card(800, 220))
    );
    let saved = serde_json::to_vec(&layout.config).unwrap();
    let restored = WindowConfig::decode(&saved).unwrap();
    assert_eq!(restored.compact, layout.config.compact);
    assert_eq!(restored.expanded, layout.config.expanded);
}

#[test]
fn dragging_back_to_original_size_does_not_keep_an_intermediate_height() {
    let mut layout = Layout::default();
    gesture(&mut layout, "South", PhysicalSize::new(316, 240));
    layout.record_manual_size(PhysicalSize::new(316, 400), 1.0);
    layout.record_manual_size(PhysicalSize::new(316, 240), 1.0);
    assert_eq!(layout.config.compact.height, Some(240.0));
}

#[test]
fn program_layout_and_temporary_menu_do_not_rewrite_manual_height() {
    let mode = ModeSize {
        width: 400.0,
        height: Some(200.0),
    };
    let bounds = Bounds::new(120.0, 1.0, None);
    assert_eq!(
        bounds.size(mode, 500.0, true, 1.0),
        PhysicalSize::new(400, 500)
    );
    assert_eq!(
        bounds.size(mode, 170.0, false, 1.0),
        PhysicalSize::new(400, 200)
    );
    let large_content = Bounds::new(350.0, 1.0, None);
    assert_eq!(
        large_content.size(mode, 500.0, false, 1.0),
        PhysicalSize::new(400, 350)
    );
    assert_eq!(mode.height, Some(200.0));
    let mut layout = Layout::default();
    layout.config.compact = mode;
    layout.record_manual_size(PhysicalSize::new(500, 500), 1.0);
    assert_eq!(layout.config.compact, mode);
}

#[test]
fn old_physical_rectangle_migrates_position_without_locking_auto_height() {
    let config = WindowConfig::decode(br#"{"x":-1200,"y":500,"width":632,"height":1052}"#).unwrap();
    assert_eq!(config.version, CONFIG_VERSION);
    assert_eq!(config.position.unwrap().x, -1200);
    assert_eq!(config.position.unwrap().y, 500);
    assert_eq!(config.compact, ModeSize::default());
    assert_eq!(config.expanded, ModeSize::default());
    assert!(WindowConfig::decode(br#"{"version":99,"position":{"x":10,"y":20}}"#).is_err());
    assert!(WindowConfig::decode(b"broken").is_err());
}

#[test]
fn default_reset_clears_both_manual_modes_and_invalidates_old_gesture() {
    let mut layout = Layout::default();
    layout.config.compact = ModeSize {
        width: 600.0,
        height: Some(400.0),
    };
    layout.config.expanded = ModeSize {
        width: 500.0,
        height: Some(700.0),
    };
    gesture(&mut layout, "SouthEast", PhysicalSize::new(600, 400));
    let generation = layout.resize_generation;
    layout.reset_sizes();
    assert!(layout.resize.is_none());
    assert_ne!(layout.resize_generation, generation);
    assert_eq!(layout.config.compact, ModeSize::default());
    assert_eq!(layout.config.expanded, ModeSize::default());
}

#[test]
fn a_tiny_work_area_takes_priority_over_normal_minimums() {
    let area = Rect {
        width: 180,
        height: 90,
        ..Rect::default()
    };
    let bounds = Bounds::new(300.0, 1.5, Some(area));
    assert!(bounds.min_width <= bounds.max_width);
    assert!(bounds.min_height <= bounds.max_height);
    let size = bounds.size(
        ModeSize {
            width: 640.0,
            height: Some(1000.0),
        },
        600.0,
        false,
        1.5,
    );
    assert_eq!(size, PhysicalSize::new(180, 90));
    let rect = Rect {
        x: 100,
        y: 80,
        width: size.width,
        height: size.height,
    }
    .clamped(area);
    assert_eq!((rect.x, rect.y), (0, 0));
}

#[test]
fn queued_dpi_layout_cannot_restore_an_old_mode_menu_or_manual_size() {
    let compact = card(800, 240);
    let mut layout = Layout::default();
    let queued_dpi = layout.dpi_request();
    // A newer user action already lays out the expanded menu at the current DPI.
    (layout, _) = layout
        .plan(
            compact,
            LayoutRequest {
                height: 650.0,
                min_height: 350.0,
                expanded: true,
                menu_open: true,
                expected_generation: None,
            },
        )
        .unwrap();
    layout.config.compact = ModeSize {
        width: 400.0,
        height: Some(300.0),
    };
    layout.config.expanded = ModeSize {
        width: 500.0,
        height: Some(700.0),
    };
    let preferences = serde_json::to_vec(&layout.config).unwrap();
    assert!(layout.plan(card(500, 700), queued_dpi).is_none());
    assert!(layout.expanded);
    assert_eq!(layout.menu, Some(compact));
    assert_eq!(layout.natural_height, 650.0);
    assert_eq!(serde_json::to_vec(&layout.config).unwrap(), preferences);
}

#[test]
fn default_reset_invalidates_queued_dpi_while_current_dpi_keeps_preferences() {
    let mut layout = Layout::default();
    layout.config.compact = ModeSize {
        width: 400.0,
        height: Some(300.0),
    };
    let queued_dpi = layout.dpi_request();
    let (current_dpi, _) = layout.plan(card(800, 300), queued_dpi).unwrap();
    assert_eq!(current_dpi.config.compact, layout.config.compact);
    assert!(!current_dpi.expanded);
    layout.reset_sizes();
    assert!(layout.plan(card(800, 300), queued_dpi).is_none());
    assert_eq!(layout.config.compact, ModeSize::default());
    assert_eq!(layout.config.expanded, ModeSize::default());
}

#[test]
fn dragging_beyond_any_work_area_edge_keeps_the_entire_card_inside() {
    let area = Rect {
        x: 0,
        y: 0,
        width: 3840,
        height: 2100,
    };
    for (x, y, expected_x, expected_y) in [
        (3300, 2200, 3300, 1735),
        (3900, 500, 3366, 500),
        (-100, 500, 0, 500),
        (1200, -100, 1200, 0),
    ] {
        let current = Rect {
            x,
            y,
            width: 474,
            height: 365,
        };
        let bounded = current.clamped(area);
        assert_eq!((bounded.x, bounded.y), (expected_x, expected_y));
        assert_eq!(bounded.clamped(area), bounded);
    }
}
