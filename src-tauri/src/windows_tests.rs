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
fn dragging_an_expanded_card_retains_the_pre_expansion_position() {
    let compact = card(800, 200);
    let dragged = Rect {
        x: 400,
        y: 300,
        ..card(600, 400)
    };
    let mut layout = Layout::default();
    layout.transition(compact, true, false);
    assert_eq!(layout.saved_rect(Some(dragged)), Some(compact));
    assert_eq!(layout.transition(dragged, false, false), Some(compact));
    assert_eq!(layout.saved_rect(Some(dragged)), Some(dragged));
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
