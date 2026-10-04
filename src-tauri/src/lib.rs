mod core;
mod desktop;
mod updates;
mod windows;

#[cfg(all(test, windows, target_env = "msvc"))]
#[link(name = "resource", kind = "static", modifiers = "-bundle")]
extern "C" {}

use core::*;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State,
};

struct TopmostMenu(CheckMenuItem<tauri::Wry>);
struct UpdateMenu(MenuItem<tauri::Wry>);

fn sync_topmost_menu(app: &tauri::AppHandle, value: bool) {
    if let Some(menu) = app.try_state::<TopmostMenu>() {
        let _ = menu.0.set_checked(value);
    }
}

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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    apply_settings(&app, &engine, settings.into(), None).await
}

async fn apply_settings(
    app: &tauri::AppHandle,
    engine: &Engine,
    patch: AppSettingsPatch,
    expected: Option<AppSettingsPatch>,
) -> Result<Bootstrap, String> {
    let _activity = app.state::<updates::Updates>().activity().await?;
    let mut update = engine.begin_settings_update(patch, expected).await?;
    let actual = windows::actual_topmost(app.clone()).await?;
    let target = update.next().always_on_top;
    if let Err(mut error) = windows::apply_topmost(app.clone(), target).await {
        if windows::apply_topmost(app.clone(), actual).await.is_err() {
            if let Ok(value) = windows::actual_topmost(app.clone()).await {
                update.reflect_actual_topmost(value);
            }
            error.push_str(" 浮窗状态恢复失败，当前置顶状态未保存，请重试。");
        }
        let state = engine.bootstrap().await;
        sync_topmost_menu(app, state.settings.always_on_top);
        publish(app, &state);
        drop(update);
        return Err(error);
    }
    match update.commit().await {
        Ok(value) => {
            sync_topmost_menu(app, value.settings.always_on_top);
            publish(app, &value);
            drop(update);
            Ok(value)
        }
        Err(error) => {
            let mut failure = error;
            if windows::apply_topmost(app.clone(), actual).await.is_err() {
                if let Ok(actual) = windows::actual_topmost(app.clone()).await {
                    update.reflect_actual_topmost(actual);
                }
                failure.push_str(" 浮窗状态恢复失败，当前置顶状态未保存，请重试。");
            }
            let value = engine.bootstrap().await;
            sync_topmost_menu(app, value.settings.always_on_top);
            publish(app, &value);
            drop(update);
            Err(failure)
        }
    }
}

#[tauri::command]
async fn patch_settings(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    patch: AppSettingsPatch,
    expected: Option<AppSettingsPatch>,
) -> Result<Bootstrap, String> {
    apply_settings(&app, &engine, patch, expected).await
}
#[tauri::command]
async fn refresh(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
    id: Option<String>,
    range: Option<UsageRange>,
    force: Option<bool>,
) -> Result<Bootstrap, String> {
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
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
    let _activity = app.state::<updates::Updates>().activity().await?;
    let value = engine.logout(id).await?;
    publish(&app, &value);
    Ok(value)
}
#[tauri::command]
async fn enable_demo(
    app: tauri::AppHandle,
    engine: State<'_, Engine>,
) -> Result<Bootstrap, String> {
    let _activity = app.state::<updates::Updates>().activity().await?;
    let value = engine.enable_demo().await?;
    publish(&app, &value);
    Ok(value)
}

pub fn run() {
    let executable = std::env::current_exe().expect("Application path is unavailable");
    let install_directory = updates::installer_directory_argument(&executable)
        .expect("Application installation path is invalid");
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !desktop::from_autostart(args) {
                let _ = windows::show_float_impl(app);
            }
        }))
        .plugin(
            tauri_plugin_updater::Builder::new()
                .installer_arg(install_directory)
                .build(),
        )
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&data)?;
            let engine = Engine::new(data.clone()).map_err(std::io::Error::other)?;
            let initial = tauri::async_runtime::block_on(engine.bootstrap());
            app.manage(engine);
            app.manage(windows::Windows::new(data.join("window.json")));
            app.manage(updates::Updates::new(
                data.join("updates.v1.json"),
                app.package_info().version.to_string(),
                updates::distribution(),
            ));
            app.state::<windows::Windows>()
                .install_native_hook(app.handle())
                .map_err(std::io::Error::other)?;
            app.state::<windows::Windows>().restore(app.handle());
            let visibility = desktop::initialize(
                app.handle(),
                data.join("desktop.v1.json"),
                std::env::current_exe()?,
                updates::distribution(),
            )
            .map_err(std::io::Error::other)?;
            if let Some(window) = app.get_webview_window("float") {
                window.set_always_on_top(initial.settings.always_on_top)?;
                if desktop::should_show(
                    desktop::from_autostart(std::env::args().skip(1)),
                    visibility,
                ) {
                    window.show()?;
                }
            }
            let show = MenuItem::with_id(app, "show", "显示浮窗", true, None::<&str>)?;
            let detail = MenuItem::with_id(app, "details", "详细用量", true, None::<&str>)?;
            let topmost = CheckMenuItem::with_id(
                app,
                "topmost",
                "浮窗置顶",
                true,
                initial.settings.always_on_top,
                None::<&str>,
            )?;
            app.manage(TopmostMenu(topmost.clone()));
            let update = MenuItem::with_id(app, "update", "检查更新", true, None::<&str>)?;
            app.manage(UpdateMenu(update.clone()));
            let quit = MenuItem::with_id(app, "quit", "退出 SubGauge", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &detail, &topmost, &update, &quit])?;
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
                    "topmost" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let engine = handle.state::<Engine>();
                            let current = engine.bootstrap().await.settings.always_on_top;
                            let patch = AppSettingsPatch {
                                always_on_top: Some(!current),
                                ..Default::default()
                            };
                            let expected = AppSettingsPatch {
                                always_on_top: Some(current),
                                ..Default::default()
                            };
                            if let Err(error) =
                                apply_settings(&handle, &engine, patch, Some(expected)).await
                            {
                                // A conflicting newer entry may already have committed.
                                // Reconcile the tray under the same serializer, never with
                                // an old failure snapshot after releasing its transaction.
                                if let Ok(guard) = engine
                                    .begin_settings_update(AppSettingsPatch::default(), None)
                                    .await
                                {
                                    let state = engine.bootstrap().await;
                                    sync_topmost_menu(&handle, state.settings.always_on_top);
                                    publish(&handle, &state);
                                    drop(guard);
                                }
                                let _ = handle.emit("subgauge:settings-error", error);
                            }
                        });
                    }
                    "update" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ =
                                windows::open_details_impl(&handle, Some("settings".into()), None);
                            updates::check_from_tray(handle).await;
                        });
                    }
                    "quit" => {
                        if app.state::<updates::Updates>().is_installing() {
                            return;
                        }
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
            updates::start_background(app.handle().clone());
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
                            let Ok(_activity) =
                                task_handle.state::<updates::Updates>().activity().await
                            else {
                                return;
                            };
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
        .on_window_event(windows::handle_event)
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            login,
            complete_2fa,
            switch_account,
            save_preferences,
            save_settings,
            patch_settings,
            refresh,
            query_records,
            analysis,
            remove_account,
            logout,
            enable_demo,
            updates::update_status,
            updates::check_update,
            updates::download_update,
            updates::install_update,
            updates::save_update_preferences,
            updates::open_update_release,
            windows::open_details,
            windows::show_float,
            windows::set_float_layout,
            windows::start_float_drag,
            windows::start_float_resize,
            windows::reset_float_size,
            desktop::desktop_status,
            desktop::save_desktop_preferences,
            desktop::open_startup_settings
        ])
        .run(tauri::generate_context!())
        .expect("SubGauge could not start");
}
