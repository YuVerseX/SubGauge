mod core;
mod windows;

use core::*;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};

fn publish(app: &tauri::AppHandle, value: &Bootstrap) {
    let _ = app.emit("subgauge:state", value);
}
#[tauri::command]
async fn bootstrap(engine: State<'_, Engine>) -> Result<Bootstrap, String> {
    Ok(engine.bootstrap().await)
}
#[tauri::command]
async fn login(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    input: LoginInput,
) -> Result<LoginOutcome, String> {
    let outcome = engine.login(input).await?;
    if let Some(value) = &outcome.state {
        publish(&app, value);
    }
    Ok(outcome)
}
#[tauri::command]
async fn complete_2fa(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    challenge_id: String,
    code: String,
) -> Result<LoginOutcome, String> {
    let outcome = engine.complete_2fa(challenge_id, code).await?;
    if let Some(value) = &outcome.state {
        publish(&app, value);
    }
    Ok(outcome)
}
#[tauri::command]
async fn switch_account(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    id: String,
) -> Result<Bootstrap, String> {
    let value = engine.switch_account(id).await?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn save_preferences(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    id: String,
    preferences: AccountPreferences,
) -> Result<Bootstrap, String> {
    let value = engine.save_preferences(id, preferences).await?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn save_settings(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    settings: AppSettings,
) -> Result<Bootstrap, String> {
    let value = engine.save_settings(settings).await?;
    windows::set_always_on_top(app.clone(), value.settings.always_on_top)?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn refresh(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    id: Option<String>,
    range: Option<UsageRange>,
    force: Option<bool>,
) -> Result<Bootstrap, String> {
    let value = engine.refresh(id, range, force.unwrap_or(false)).await?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn query_records(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    query: RecordQuery,
) -> Result<RecordPage, String> {
    let result = engine.query_records(query).await;
    publish(&app, &engine.bootstrap().await);
    result
}
#[tauri::command]
async fn analysis(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    query: AnalysisQuery,
) -> Result<AnalysisResult, String> {
    let result = engine.analysis(query).await;
    publish(&app, &engine.bootstrap().await);
    result
}
#[tauri::command]
async fn remove_account(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    id: String,
) -> Result<Bootstrap, String> {
    let value = engine.remove_account(id).await?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn logout(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    id: String,
) -> Result<Bootstrap, String> {
    let value = engine.logout(id).await?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn enable_demo(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
) -> Result<Bootstrap, String> {
    let value = engine.enable_demo().await?;
    publish(&app, &value);
    Ok(value)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = windows::show_float_impl(app);
        }))
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let engine = Engine::new(data.clone()).map_err(std::io::Error::other)?;
            let initial = tauri::async_runtime::block_on(engine.bootstrap());
            app.manage(engine);
            app.manage(windows::Windows::new(data.join("window.json")));
            app.state::<windows::Windows>().restore(app.handle());
            if let Some(window) = app.get_webview_window("float") {
                window.set_always_on_top(initial.settings.always_on_top)?;
                window.show()?;
            }
            let show = MenuItem::with_id(app, "show", "显示浮窗", true, None::<&str>)?;
            let detail = MenuItem::with_id(app, "details", "详细用量", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 SubGauge", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &detail, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("SubGauge")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        let _ = windows::show_float_impl(app);
                    }
                    "details" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = windows::open_details_impl(&handle, None, None);
                        });
                    }
                    "quit" => {
                        app.state::<windows::Windows>().save(app);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let _ = windows::show_float_impl(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(2));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    let ids = handle.state::<Engine>().account_ids();
                    for id in ids {
                        let task_handle = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let engine = task_handle.state::<Engine>();
                            if engine.refresh_due(id).await {
                                publish(&task_handle, &engine.bootstrap().await);
                            }
                        });
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "float" {
                match event {
                    tauri::WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    tauri::WindowEvent::Moved(_) => window
                        .app_handle()
                        .state::<windows::Windows>()
                        .on_moved(window.app_handle()),
                    tauri::WindowEvent::Resized(size) => window
                        .app_handle()
                        .state::<windows::Windows>()
                        .on_resized(window.app_handle(), *size),
                    tauri::WindowEvent::ScaleFactorChanged { .. } => window
                        .app_handle()
                        .state::<windows::Windows>()
                        .on_scale_changed(window.app_handle()),
                    _ => {}
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            login,
            complete_2fa,
            switch_account,
            save_preferences,
            save_settings,
            refresh,
            query_records,
            analysis,
            remove_account,
            logout,
            enable_demo,
            windows::open_details,
            windows::show_float,
            windows::set_float_layout,
            windows::start_float_resize,
            windows::reset_float_size
        ])
        .run(tauri::generate_context!())
        .expect("SubGauge could not start");
}
