use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

type Reply = (u16, Value, Vec<(String, String)>);
async fn server(
    handler: impl Fn(String) -> Reply + Send + Sync + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handler = Arc::new(handler);
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                break;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let mut bytes = vec![];
                loop {
                    let mut buf = [0_u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some(header_end) = text.find("\r\n\r\n") {
                        let content = text[..header_end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|n| n.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= header_end + 4 + content {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&bytes).to_string();
                let (status, mut body, headers) = handler(request.clone());
                if let Some(data) = body.get_mut("data").filter(|v| v.get("items").is_some()) {
                    let target = request.split_whitespace().nth(1).unwrap();
                    let url = url::Url::parse(&format!("http://fixture{target}")).unwrap();
                    let params = url.query_pairs().collect::<HashMap<_, _>>();
                    for name in ["page", "page_size"] {
                        if data.get(name).is_none() {
                            data[name] = json!(params.get(name).unwrap().parse::<u64>().unwrap());
                        }
                    }
                }
                let payload = serde_json::to_vec(&body).unwrap();
                let headers = headers
                    .iter()
                    .map(|(k, v)| format!("{k}: {v}\r\n"))
                    .collect::<String>();
                let head=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",payload.len());
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&payload).await;
                // Complete a graceful TCP half-close before dropping the Windows socket.
                // Otherwise a fast mock can reset the connection while the client still
                // consumes the response, producing intermittent transport failures.
                let _ = socket.shutdown().await;
                let mut tail = [0_u8; 128];
                let _ =
                    tokio::time::timeout(std::time::Duration::from_secs(1), socket.read(&mut tail))
                        .await;
            });
        }
    });
    (format!("http://{address}"), task)
}
fn ok(value: Value) -> Reply {
    (200, json!({"code":0,"data":value}), vec![])
}
fn account(site: String, id: &str) -> AccountSummary {
    AccountSummary {
        id: id.into(),
        site,
        email: "test@example.com".into(),
        role: "user".into(),
        user_id: 7,
        preferences: AccountPreferences::default(),
        needs_login: false,
        demo: false,
    }
}
fn session(expired: bool) -> Session {
    Session {
        access_token: "test-access".into(),
        refresh_token: Some("test-refresh".into()),
        expires_at: Utc::now().timestamp() + if expired { -10 } else { 3600 },
    }
}
fn engine() -> Engine {
    Engine::new(std::env::temp_dir().join(format!("subgauge-test-{}", uuid::Uuid::new_v4())))
        .unwrap()
}
fn seed(engine: &Engine, a: AccountSummary, session: Session) -> Arc<Runtime> {
    let rt = Arc::new(Runtime::new(Some(session), UsageRange::Today));
    let mut state = engine.state.lock().unwrap();
    state.runtime.insert(a.id.clone(), rt.clone());
    state.config.current = Some(a.id.clone());
    state.config.accounts.push(SavedAccount {
        summary: a,
        encrypted_session: None,
        snapshot: None,
    });
    rt
}
fn cleanup(engine: &Engine) {
    if let Some(dir) = engine.path.parent() {
        let _ = std::fs::remove_dir_all(dir);
    }
}

fn preference_analysis_fixture() -> AnalysisResult {
    AnalysisResult {
        account_id: "preferences-fixture".into(),
        generation: 0,
        range: UsageRange::Today,
        start: "2026-10-01T00:00:00Z".into(),
        end: "2026-10-02T00:00:00Z".into(),
        timezone: "Asia/Shanghai".into(),
        trend: vec![],
        trend_meta: TrendMetadata {
            source: "server".into(),
            granularity: "hour".into(),
            timezone: None,
            missing_buckets: "unknown".into(),
            complete: true,
        },
        models: vec![],
        keys: vec![],
        complete: true,
        message: None,
        synced_at: "2026-10-02T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn save_preferences_rejects_runtime_replaced_while_waiting_for_analysis() {
    let e = engine();
    let mut a = account("https://fixture.example".into(), "preferences-fixture");
    a.preferences.alias = "Original fixture".into();
    let old = seed(&e, a.clone(), session(false));
    *old.last_summary.lock().unwrap() = 101;
    *old.last_recent.lock().unwrap() = 102;
    let mut old_cache = old.analysis.lock().await;
    *old_cache = Some(("old fixture".into(), 103, preference_analysis_fixture()));
    let expected_old_cache = serde_json::to_value(&*old_cache).unwrap();
    let mut preferences = a.preferences.clone();
    preferences.alias = "Requested change".into();
    preferences.default_range = UsageRange::Month;
    let mut saving = std::pin::pin!(e.save_preferences(a.id.clone(), preferences));
    // Poll exactly to the held analysis lock, without scheduler timing assumptions.
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(saving.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let new = Arc::new(Runtime::new(Some(session(false)), UsageRange::Today));
    *new.last_summary.lock().unwrap() = 201;
    *new.last_recent.lock().unwrap() = 202;
    *new.analysis.lock().await = Some(("new fixture".into(), 203, preference_analysis_fixture()));
    let expected_new_cache = serde_json::to_value(&*new.analysis.lock().await).unwrap();
    let snapshot = e.demo_snapshot(&a, UsageRange::Today, 11).unwrap();
    let expected_config = {
        let mut state = e.state.lock().unwrap();
        state.runtime.insert(a.id.clone(), new.clone());
        state.config.accounts[0].snapshot = Some(snapshot);
        state.generation = 11;
        serde_json::to_value(&state.config).unwrap()
    };
    e.persist().unwrap();
    let expected_disk = std::fs::read(&e.path).unwrap();
    drop(old_cache);
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(1), saving)
        .await
        .expect("saving must finish after the analysis lock is released");
    let state = e.bootstrap().await;
    let current_config = serde_json::to_value(&e.state.lock().unwrap().config).unwrap();
    let current_disk = std::fs::read(&e.path).unwrap();
    let old_cache = serde_json::to_value(&*old.analysis.lock().await).unwrap();
    let new_cache = serde_json::to_value(&*new.analysis.lock().await).unwrap();
    cleanup(&e);
    let error = outcome.expect_err("a replaced runtime must reject the stale save");
    assert!(error.contains("会话已变更") && error.contains("重试"));
    assert_eq!(current_config, expected_config);
    assert_eq!(current_disk, expected_disk);
    assert_eq!(state.generation, 11);
    assert_eq!(state.selected_range, UsageRange::Today);
    assert_eq!(state.accounts[0].preferences, a.preferences);
    assert_eq!(old_cache, expected_old_cache);
    assert_eq!(new_cache, expected_new_cache);
    assert_eq!(*old.range.lock().unwrap(), UsageRange::Today);
    assert_eq!(*old.last_summary.lock().unwrap(), 101);
    assert_eq!(*old.last_recent.lock().unwrap(), 102);
    assert_eq!(*new.range.lock().unwrap(), UsageRange::Today);
    assert_eq!(*new.last_summary.lock().unwrap(), 201);
    assert_eq!(*new.last_recent.lock().unwrap(), 202);
}

#[tokio::test]
async fn save_preferences_updates_current_runtime_and_persists_after_restart() {
    let e = engine();
    let mut a = account("https://fixture.example".into(), "preferences-fixture");
    a.preferences.alias = "Original fixture".into();
    let rt = seed(&e, a.clone(), session(false));
    *rt.last_summary.lock().unwrap() = 101;
    *rt.last_recent.lock().unwrap() = 102;
    *rt.analysis.lock().await = Some(("fixture".into(), 103, preference_analysis_fixture()));
    e.state.lock().unwrap().config.accounts[0].snapshot =
        Some(e.demo_snapshot(&a, UsageRange::Today, 0).unwrap());
    let preferences = AccountPreferences {
        alias: "Saved fixture".into(),
        default_range: UsageRange::Month,
        metrics: vec!["balance".into(), "cost".into()],
        recent_minutes: 15,
        timezone: "UTC".into(),
    };
    let state = e
        .save_preferences(a.id.clone(), preferences.clone())
        .await
        .unwrap();
    assert_eq!(state.generation, 1);
    assert_eq!(state.selected_range, preferences.default_range);
    assert_eq!(state.accounts[0].preferences, preferences);
    assert!(state.snapshot.is_none());
    assert!(rt.analysis.lock().await.is_none());
    assert_eq!(*rt.last_summary.lock().unwrap(), 0);
    assert_eq!(*rt.last_recent.lock().unwrap(), 0);
    let restarted = Engine::new(e.path.parent().unwrap().to_owned()).unwrap();
    let state = restarted.bootstrap().await;
    assert_eq!(state.current_account_id, Some(a.id));
    assert_eq!(state.selected_range, preferences.default_range);
    assert_eq!(state.accounts[0].preferences, preferences);
    assert!(state.snapshot.is_none());
    cleanup(&e);
}

#[cfg(windows)]
#[tokio::test]
async fn legacy_settings_and_theme_changes_preserve_account_configuration() {
    let e = engine();
    let mut saved_account = account("https://fixture.example".into(), "theme-fixture");
    saved_account.preferences.alias = "Fixture".into();
    saved_account.preferences.default_range = UsageRange::Week;
    let encrypted_session =
        storage::protect(&serde_json::to_vec(&session(false)).unwrap()).unwrap();
    let config = Config {
        accounts: vec![SavedAccount {
            summary: saved_account.clone(),
            encrypted_session: Some(encrypted_session.clone()),
            snapshot: None,
        }],
        current: Some(saved_account.id.clone()),
        settings: AppSettings {
            opacity: 0.8,
            always_on_top: false,
            ..AppSettings::default()
        },
        ..Config::default()
    };
    let mut legacy = serde_json::to_value(config).unwrap();
    legacy["settings"].as_object_mut().unwrap().remove("theme");
    storage::atomic_write(&e.path, &serde_json::to_vec(&legacy).unwrap()).unwrap();
    let restored = Engine::new(e.path.parent().unwrap().to_owned()).unwrap();
    let state = restored.bootstrap().await;
    assert_eq!(state.settings.theme, AppearanceTheme::Light);
    assert_eq!(state.settings.opacity, 0.8);
    assert!(!state.settings.always_on_top);
    assert_eq!(state.current_account_id, Some(saved_account.id.clone()));
    assert!(!state.accounts[0].needs_login);
    for theme in [AppearanceTheme::Dark, AppearanceTheme::System] {
        let settings = AppSettings {
            theme,
            ..state.settings.clone()
        };
        restored.save_settings(settings).await.unwrap();
        let restarted = Engine::new(e.path.parent().unwrap().to_owned()).unwrap();
        let state = restarted.bootstrap().await;
        assert_eq!(state.settings.theme, theme);
        assert_eq!(state.settings.opacity, 0.8);
        assert_eq!(state.accounts[0].preferences, saved_account.preferences);
        assert_eq!(state.current_account_id, Some(saved_account.id.clone()));
        assert!(!state.accounts[0].needs_login);
        assert_eq!(
            restarted.state.lock().unwrap().config.accounts[0].encrypted_session,
            Some(encrypted_session.clone())
        );
    }
    cleanup(&e);
}

#[tokio::test]
async fn settings_patch_preserves_unrelated_fields_and_account_changes() {
    let e = engine();
    let a = account("https://fixture.example".into(), "settings-fixture");
    seed(&e, a.clone(), session(false));
    let mut update = e
        .begin_settings_update(
            AppSettingsPatch {
                theme: Some(AppearanceTheme::Dark),
                ..AppSettingsPatch::default()
            },
            Some(AppSettingsPatch {
                theme: Some(AppearanceTheme::Light),
                // Unsubmitted fields must not participate in conflict detection.
                always_on_top: Some(false),
                ..AppSettingsPatch::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(update.previous().theme, AppearanceTheme::Light);
    assert_eq!(update.next().theme, AppearanceTheme::Dark);
    assert_eq!(e.bootstrap().await.settings.theme, AppearanceTheme::Light);
    {
        // An account update can finish while the native settings effect is pending.
        let mut state = e.state.lock().unwrap();
        state.config.accounts[0].summary.preferences.alias = "Updated account".into();
        state.generation += 1;
    }
    let saved = update.commit().await.unwrap();
    assert_eq!(saved.generation, 2);
    assert_eq!(saved.settings.theme, AppearanceTheme::Dark);
    assert!(saved.settings.always_on_top);
    assert_eq!(saved.settings.recent_refresh_seconds, 10);
    assert_eq!(saved.accounts[0].preferences.alias, "Updated account");
    let restarted = Engine::new(e.path.parent().unwrap().to_owned()).unwrap();
    let saved = restarted.bootstrap().await;
    assert_eq!(saved.settings.theme, AppearanceTheme::Dark);
    assert_eq!(saved.accounts[0].preferences.alias, "Updated account");
    cleanup(&e);
}

#[tokio::test]
async fn settings_transactions_serialize_without_overwriting_another_field() {
    let e = engine();
    let mut first = e
        .begin_settings_update(
            AppSettingsPatch {
                always_on_top: Some(false),
                ..AppSettingsPatch::default()
            },
            Some(AppSettingsPatch {
                always_on_top: Some(true),
                ..AppSettingsPatch::default()
            }),
        )
        .await
        .unwrap();
    let mut second = std::pin::pin!(e.begin_settings_update(
        AppSettingsPatch {
            theme: Some(AppearanceTheme::Dark),
            ..AppSettingsPatch::default()
        },
        Some(AppSettingsPatch {
            theme: Some(AppearanceTheme::Light),
            ..AppSettingsPatch::default()
        }),
    ));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(second.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let old_response = e.bootstrap().await;
    let first_response = first.commit().await.unwrap();
    assert_eq!(first_response.generation, old_response.generation + 1);
    // Keep serial ownership even after commit until native/tray reconciliation ends.
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(second.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(first);
    let mut second = second.await.unwrap();
    let latest = second.commit().await.unwrap();
    assert_eq!(latest.generation, first_response.generation + 1);
    assert_eq!(latest.settings.theme, AppearanceTheme::Dark);
    assert!(!latest.settings.always_on_top);
    let restarted = Engine::new(e.path.parent().unwrap().to_owned()).unwrap();
    assert_eq!(restarted.bootstrap().await.settings, latest.settings);
    cleanup(&e);
}

#[tokio::test]
async fn settings_patch_rejects_same_field_conflict_and_missing_baseline() {
    let e = engine();
    let mut first = e
        .begin_settings_update(
            AppSettingsPatch {
                theme: Some(AppearanceTheme::Dark),
                ..AppSettingsPatch::default()
            },
            None,
        )
        .await
        .unwrap();
    first.commit().await.unwrap();
    drop(first);
    let expected_disk = std::fs::read(&e.path).unwrap();
    let patch = AppSettingsPatch {
        theme: Some(AppearanceTheme::System),
        ..AppSettingsPatch::default()
    };
    let conflict = e
        .begin_settings_update(
            patch.clone(),
            Some(AppSettingsPatch {
                theme: Some(AppearanceTheme::Light),
                ..AppSettingsPatch::default()
            }),
        )
        .await
        .err()
        .expect("a stale edit of the same field must conflict");
    assert!(conflict.contains("主题") && conflict.contains("重新确认"));
    let missing = e
        .begin_settings_update(patch, Some(AppSettingsPatch::default()))
        .await
        .err()
        .expect("each changed field requires its editing baseline");
    assert!(missing.contains("原值"));
    assert_eq!(e.bootstrap().await.generation, 1);
    assert_eq!(std::fs::read(&e.path).unwrap(), expected_disk);
    cleanup(&e);
}

#[cfg(windows)]
#[tokio::test]
async fn settings_save_failure_retains_old_state_and_serializes_native_rollback() {
    use std::os::windows::fs::OpenOptionsExt;
    let e = engine();
    e.persist().unwrap();
    let old_disk = std::fs::read(&e.path).unwrap();
    // Deny replacement of this isolated fixture; do not alter actual user files.
    let locked_file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&e.path)
        .unwrap();
    let mut update = e
        .begin_settings_update(
            AppSettingsPatch {
                always_on_top: Some(false),
                ..AppSettingsPatch::default()
            },
            None,
        )
        .await
        .unwrap();
    assert!(update.commit().await.is_err());
    let state = e.bootstrap().await;
    assert_eq!(state.settings, AppSettings::default());
    assert_eq!(state.generation, 0);
    let mut next = std::pin::pin!(e.begin_settings_update(AppSettingsPatch::default(), None));
    std::future::poll_fn(|cx| {
        assert!(std::future::Future::poll(next.as_mut(), cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(locked_file);
    assert_eq!(std::fs::read(&e.path).unwrap(), old_disk);
    drop(update);
    drop(next.await.unwrap());
    let leftovers = std::fs::read_dir(e.path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(
        leftovers,
        vec![std::ffi::OsString::from("accounts.v1.json")]
    );
    cleanup(&e);
}

#[tokio::test]
async fn failed_native_rollback_reports_actual_state_without_background_persistence() {
    let e = engine();
    e.persist().unwrap();
    let mut update = e
        .begin_settings_update(
            AppSettingsPatch {
                always_on_top: Some(false),
                ..AppSettingsPatch::default()
            },
            None,
        )
        .await
        .unwrap();
    update.reflect_actual_topmost(false);
    let actual = e.bootstrap().await;
    assert!(!actual.settings.always_on_top);
    assert_eq!(actual.generation, 1);
    e.persist().unwrap();
    let on_disk: Config = serde_json::from_slice(&std::fs::read(&e.path).unwrap()).unwrap();
    assert!(on_disk.settings.always_on_top);
    drop(update);
    let mut retry = e
        .begin_settings_update(
            AppSettingsPatch {
                always_on_top: Some(false),
                ..AppSettingsPatch::default()
            },
            Some(AppSettingsPatch {
                always_on_top: Some(false),
                ..AppSettingsPatch::default()
            }),
        )
        .await
        .unwrap();
    let saved = retry.commit().await.unwrap();
    assert!(!saved.settings.always_on_top);
    assert_eq!(saved.generation, 2);
    let restarted = Engine::new(e.path.parent().unwrap().to_owned()).unwrap();
    assert!(!restarted.bootstrap().await.settings.always_on_top);
    assert!(e.state.lock().unwrap().actual_topmost.is_none());
    cleanup(&e);
}

#[tokio::test]
async fn settings_noop_and_invalid_values_do_not_create_a_new_generation() {
    let e = engine();
    let mut update = e
        .begin_settings_update(AppSettingsPatch::default(), None)
        .await
        .unwrap();
    assert_eq!(update.commit().await.unwrap().generation, 0);
    assert!(!e.path.exists());
    drop(update);
    for opacity in [f64::NAN, 0.1, 1.1] {
        assert!(e
            .begin_settings_update(
                AppSettingsPatch {
                    opacity: Some(opacity),
                    ..AppSettingsPatch::default()
                },
                None,
            )
            .await
            .is_err());
    }
    assert_eq!(e.bootstrap().await.generation, 0);
    assert_eq!(e.bootstrap().await.settings, AppSettings::default());
    cleanup(&e);
}

#[test]
fn unknown_theme_preserves_original_config() {
    let e = engine();
    let mut config = serde_json::to_value(Config::default()).unwrap();
    config["settings"]["theme"] = json!("future-theme");
    let bytes = serde_json::to_vec(&config).unwrap();
    storage::atomic_write(&e.path, &bytes).unwrap();
    assert!(Engine::new(e.path.parent().unwrap().to_owned()).is_err());
    assert_eq!(std::fs::read(&e.path).unwrap(), bytes);
    cleanup(&e);
}
fn row(id: i64, time: DateTime<Utc>) -> Value {
    json!({"id":id,"created_at":time.to_rfc3339(),"model":"test","api_key_id":1,"input_tokens":100,"output_tokens":10,"cache_read_tokens":20,"cache_creation_tokens":30,"actual_cost":0.01,"api_key":{"name":"test"}})
}

#[tokio::test]
async fn concurrent_requests_refresh_one_session_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (site,server)=server(move|request|{if request.starts_with("POST /api/v1/auth/refresh"){observed.fetch_add(1,Ordering::SeqCst);ok(json!({"access_token":"new-test-access","refresh_token":"new-test-refresh","expires_in":3600}))}else{assert!(request.to_lowercase().contains("authorization: bearer new-test-access"));ok(json!({"balance":10}))}}).await;
    let engine = Arc::new(engine());
    let a = account(site, "first");
    let rt = seed(&engine, a.clone(), session(true));
    let (left, right) = tokio::join!(
        engine.get(&a, &rt, "/user/profile", &[]),
        engine.get(&a, &rt, "/user/profile", &[])
    );
    assert!(left.is_ok() && right.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn old_session_failure_does_not_invalidate_relogin() {
    let (site, server) = server(|_| {
        (
            401,
            json!({"code":401,"message":"invalid test token"}),
            vec![],
        )
    })
    .await;
    let engine = engine();
    let a = account(site, "first");
    let old = seed(&engine, a.clone(), session(false));
    let new = Arc::new(Runtime::new(Some(session(false)), UsageRange::Today));
    engine
        .state
        .lock()
        .unwrap()
        .runtime
        .insert(a.id.clone(), new.clone());
    assert!(engine.get(&a, &old, "/user/profile", &[]).await.is_err());
    assert!(!engine.bootstrap().await.accounts[0].needs_login);
    assert!(new.session.lock().await.is_some());
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn failed_account_does_not_invalidate_other_site() {
    let (site, server) = server(|_| (401, json!({"code":401}), vec![])).await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    seed(
        &engine,
        account("https://other.example.com".into(), "b"),
        session(false),
    );
    assert!(engine.get(&a, &rt, "/user/profile", &[]).await.is_err());
    let state = engine.bootstrap().await;
    assert!(state.accounts[0].needs_login);
    assert!(!state.accounts[1].needs_login);
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn redirect_never_forwards_credentials() {
    let hits = Arc::new(AtomicUsize::new(0));
    let count = hits.clone();
    let (target, target_task) = server(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        ok(json!({}))
    })
    .await;
    let (site, source) =
        server(move |_| (302, json!({}), vec![("Location".into(), target.clone())])).await;
    let api = Api::new().unwrap();
    let result = api
        .raw(&site, "/user/profile", Some("test-secret"), None, &[])
        .await;
    assert!(result.unwrap_err().message.contains("重定向"));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    source.abort();
    target_task.abort();
}
#[tokio::test]
async fn limit_error_is_redacted_and_keeps_retry_after() {
    let (site, server) = server(|_| {
        (
            429,
            json!({"message":"test-secret"}),
            vec![("Retry-After".into(), "83".into())],
        )
    })
    .await;
    let api = Api::new().unwrap();
    let error = api.raw(&site, "/usage", None, None, &[]).await.unwrap_err();
    assert_eq!(error.retry_after, Some(83));
    assert!(!error.message.contains("test-secret"));
    server.abort();
}
#[tokio::test]
async fn detail_rate_limit_blocks_concurrent_reads_but_keeps_cache_and_other_accounts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (site, limited_server) = server(move |request| {
        let call = observed.fetch_add(1, Ordering::SeqCst);
        if call == 2 {
            return (429, json!({}), vec![("Retry-After".into(), "60".into())]);
        }
        if request.contains("/usage/dashboard/trend") {
            ok(json!({"trend":[]}))
        } else if request.contains("/usage/dashboard/models") {
            ok(json!({"models":[]}))
        } else {
            ok(json!({"items":[],"total":0}))
        }
    })
    .await;
    let (healthy_site, healthy_server) = server(|_| ok(json!({"balance":10}))).await;
    let engine = engine();
    let limited = account(site, "limited");
    let rt = seed(&engine, limited.clone(), session(false));
    let mut saved_snapshot = engine
        .demo_snapshot(&limited, UsageRange::Today, 0)
        .unwrap();
    saved_snapshot.demo = false;
    let last_totals = serde_json::to_value(&saved_snapshot.totals).unwrap();
    engine.state.lock().unwrap().config.accounts[0].snapshot = Some(saved_snapshot);
    let healthy = account(healthy_site, "healthy");
    let healthy_rt = seed(&engine, healthy.clone(), session(false));
    let analysis = AnalysisQuery {
        account_id: "limited".into(),
        range: UsageRange::Today,
        api_key_id: None,
        model: None,
        include_keys: false,
        force: false,
    };
    let cached = engine.analysis(analysis.clone()).await.unwrap();
    let records = RecordQuery {
        account_id: "limited".into(),
        range: UsageRange::Today,
        page: 1,
        page_size: 20,
        api_key_id: None,
        model: None,
    };
    let (first, second, forced_analysis) = tokio::join!(
        engine.query_records(records.clone()),
        engine.query_records(records.clone()),
        engine.analysis(AnalysisQuery {
            force: true,
            ..analysis.clone()
        })
    );
    assert!(first.is_err() && second.is_err() && forced_analysis.is_err());
    {
        let state = engine.state.lock().unwrap();
        let snapshot = state.config.accounts[0].snapshot.as_ref().unwrap();
        assert_eq!(snapshot.sync.state, "stale");
        assert!(snapshot.sync.message.as_ref().unwrap().contains("限流"));
        assert_eq!(serde_json::to_value(&snapshot.totals).unwrap(), last_totals);
    }
    engine.refresh_account("limited", true).await.unwrap();
    let still_cached = engine.analysis(analysis).await.unwrap();
    assert_eq!(still_cached.synced_at, cached.synced_at);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(*rt.retry_at.lock().unwrap() >= Utc::now().timestamp() + 58);
    assert!(engine
        .get(&healthy, &healthy_rt, "/user/profile", &[])
        .await
        .is_ok());
    // Advance just this account's deadline without sleeping or changing real clocks.
    *rt.retry_at.lock().unwrap() = Utc::now().timestamp() - 1;
    assert!(engine.query_records(records).await.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    limited_server.abort();
    healthy_server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn background_rate_limit_stops_requests_in_the_same_refresh_cycle() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (site, server) = server(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        (429, json!({}), vec![("Retry-After".into(), "60".into())])
    })
    .await;
    let engine = engine();
    let rt = seed(&engine, account(site, "limited"), session(false));
    engine.refresh_account("limited", true).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(*rt.retry_at.lock().unwrap() >= Utc::now().timestamp() + 58);
    let snapshot = engine.bootstrap().await.snapshot.unwrap();
    assert_eq!(snapshot.sync.state, "stale");
    assert!(snapshot.sync.message.unwrap().contains("限流"));
    engine.refresh_account("limited", true).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn pagination_overlap_is_deduplicated_and_revalidated() {
    let now = Utc::now();
    let page_calls = Arc::new(AtomicUsize::new(0));
    let count = page_calls.clone();
    let (site,server)=server(move|request|{count.fetch_add(1,Ordering::SeqCst);let ids:Vec<i64>=if request.contains("page=2&"){(1..=51).rev().collect()}else{(51..=150).rev().collect()};ok(json!({"items":ids.into_iter().map(|id|row(id,now-Duration::seconds(151-id))).collect::<Vec<_>>(),"total":150}))}).await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    let (records, complete) = engine
        .collect(&a, &rt, now - Duration::minutes(5), now, None, None)
        .await
        .unwrap();
    assert_eq!(records.len(), 150);
    assert!(complete);
    assert_eq!(page_calls.load(Ordering::SeqCst), 3);
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn changing_head_is_never_marked_complete() {
    let now = Utc::now();
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let (site, server) = server(move |_| {
        let id = observed.fetch_add(1, Ordering::SeqCst) as i64;
        ok(json!({"items":[row(id,now-Duration::seconds(1))],"total":1}))
    })
    .await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    let (_, complete) = engine
        .collect(&a, &rt, now - Duration::minutes(5), now, None, None)
        .await
        .unwrap();
    assert!(!complete);
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn fresh_install_and_demo_are_explicit() {
    let engine = engine();
    assert!(engine.bootstrap().await.accounts.is_empty());
    let state = engine.enable_demo().await.unwrap();
    assert!(state.snapshot.unwrap().demo);
    assert_eq!(state.accounts.len(), 2);
    engine.persist().unwrap();
    let persisted: Config = serde_json::from_slice(&std::fs::read(&engine.path).unwrap()).unwrap();
    assert!(persisted.accounts.is_empty());
    cleanup(&engine);
}
#[tokio::test]
async fn selected_range_never_returns_a_different_snapshot() {
    let engine = engine();
    engine.enable_demo().await.unwrap();
    let (_, rt) = engine.context("demo:1").unwrap();
    *rt.range.lock().unwrap() = UsageRange::Month;
    let state = engine.bootstrap().await;
    assert_eq!(state.selected_range, UsageRange::Month);
    assert!(state.snapshot.is_none());
    cleanup(&engine);
}

#[tokio::test]
async fn analysis_keeps_sample_metadata_and_manual_refresh_bypasses_cache() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = calls.clone();
    let (site, server) = server(move |request| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if request.contains("/usage/dashboard/trend") {
            ok(json!({"trend":[]}))
        } else {
            ok(json!({"models":[]}))
        }
    })
    .await;
    let engine = engine();
    seed(&engine, account(site, "a"), session(false));
    let query = AnalysisQuery {
        account_id: "a".into(),
        range: UsageRange::Month,
        api_key_id: None,
        model: None,
        include_keys: false,
        force: false,
    };
    let first = engine.analysis(query.clone()).await.unwrap();
    assert_eq!(first.range, UsageRange::Month);
    assert_eq!(first.timezone, "Asia/Shanghai");
    let start = DateTime::parse_from_rfc3339(&first.start).unwrap();
    let end = DateTime::parse_from_rfc3339(&first.end).unwrap();
    assert!(start < end);
    assert!(first.complete);
    assert_eq!(first.trend_meta.source, "server");
    assert!(first.trend_meta.timezone.is_none());
    assert_eq!(first.trend_meta.missing_buckets, "unknown");
    let cached = engine.analysis(query.clone()).await.unwrap();
    assert_eq!(cached.start, first.start);
    assert_eq!(cached.end, first.end);
    assert_eq!(cached.synced_at, first.synced_at);
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    let refreshed = engine
        .analysis(AnalysisQuery {
            force: true,
            ..query
        })
        .await
        .unwrap();
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 4);
    assert!(DateTime::parse_from_rfc3339(&refreshed.end).unwrap() >= end);
    assert_eq!(refreshed.range, UsageRange::Month);
    server.abort();
    cleanup(&engine);
}

#[tokio::test]
async fn totp_challenge_stays_native_and_failed_login_is_not_saved() {
    let (site,server)=server(|request|{
        if request.starts_with("POST /api/v1/auth/login/2fa") {assert!(request.contains("test-temp-token"));assert!(request.contains("totp_code"));return ok(json!({"access_token":"test-access","expires_in":3600}));}
        if request.starts_with("POST /api/v1/auth/login") {return ok(json!({"requires_2fa":true,"temp_token":"test-temp-token","user_email_masked":"t***@example.com"}));}
        ok(json!({"id":7,"email":"test@example.com","role":"user","balance":12.3}))
    }).await;
    let engine = engine();
    let result = engine
        .login(LoginInput {
            site,
            email: "test@example.com".into(),
            password: "test-password".into(),
            alias: "测试".into(),
            remember: false,
        })
        .await
        .unwrap();
    assert_eq!(result.status, "twoFactor");
    assert!(engine.bootstrap().await.accounts.is_empty());
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("test-temp-token"));
    assert!(!serialized.contains("test-password"));
    let result = engine
        .complete_2fa(result.challenge_id.unwrap(), "123456".into())
        .await
        .unwrap();
    assert_eq!(result.status, "success");
    assert_eq!(result.state.unwrap().accounts[0].role, "user");
    let disk = std::fs::read_to_string(&engine.path).unwrap();
    assert!(!disk.contains("test-access"));
    assert!(!disk.contains("test-password"));
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn midnight_end_does_not_request_an_extra_day() {
    let start = DateTime::parse_from_rfc3339("2026-09-24T06:30:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let end = DateTime::parse_from_rfc3339("2026-09-24T16:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let (site, server) = server(|request| {
        assert!(request.contains("end_date=2026-09-24"));
        assert!(!request.contains("end_date=2026-09-25"));
        ok(json!({"items":[],"total":0}))
    })
    .await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    assert!(
        engine
            .collect(&a, &rt, start, end, None, None)
            .await
            .unwrap()
            .1
    );
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn duplicate_pages_with_missing_records_are_incomplete() {
    let now = Utc::now();
    let (site,server)=server(move|request|{let ids:Vec<i64>=if request.contains("page=2&"){(1..=51).rev().collect()}else{(51..=150).rev().collect()};ok(json!({"items":ids.into_iter().map(|id|row(id,now-Duration::seconds(151-id))).collect::<Vec<_>>(),"total":151}))}).await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    let (records, complete) = engine
        .collect(&a, &rt, now - Duration::minutes(5), now, None, None)
        .await
        .unwrap();
    assert_eq!(records.len(), 150);
    assert!(!complete);
    server.abort();
    cleanup(&engine);
}
#[tokio::test]
async fn recent_success_cannot_clear_failed_summary() {
    let summaries = Arc::new(AtomicUsize::new(0));
    let count = summaries.clone();
    let (site, server) = server(move |request| {
        if request.starts_with("GET /api/v1/usage/stats") {
            count.fetch_add(1, Ordering::SeqCst);
            return (500, json!({"code":500}), vec![]);
        }
        if request.starts_with("GET /api/v1/user/profile") {
            return ok(json!({"balance":10}));
        }
        ok(json!({"items":[],"total":0}))
    })
    .await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    engine.refresh_account("a", true).await.unwrap();
    assert_eq!(
        engine.bootstrap().await.snapshot.unwrap().sync.state,
        "stale"
    );
    *rt.retry_at.lock().unwrap() = 0;
    *rt.last_recent.lock().unwrap() = 0;
    engine.refresh_account("a", false).await.unwrap();
    assert_eq!(summaries.load(Ordering::SeqCst), 2);
    assert_eq!(
        engine.bootstrap().await.snapshot.unwrap().sync.state,
        "stale"
    );
    server.abort();
    cleanup(&engine);
}

#[tokio::test]
async fn incomplete_recent_window_does_not_taint_today_or_force_summary_reads() {
    let changing = Arc::new(AtomicUsize::new(1));
    let recent_calls = Arc::new(AtomicUsize::new(0));
    let summary_calls = Arc::new(AtomicUsize::new(0));
    let unstable = changing.clone();
    let counts = recent_calls.clone();
    let summaries = summary_calls.clone();
    let now = Utc::now() - Duration::seconds(1);
    let (site, task) = server(move |request| {
        if request.starts_with("GET /api/v1/usage/stats") {
            summaries.fetch_add(1, Ordering::SeqCst);
            return ok(json!({"total_requests":42,"total_actual_cost":"2.34",
                "total_input_tokens":0,"total_output_tokens":0,
                "total_cache_read_tokens":0,"total_cache_creation_tokens":0}));
        }
        if request.starts_with("GET /api/v1/user/profile") {
            return ok(json!({"balance":10}));
        }
        let serial = counts.fetch_add(1, Ordering::SeqCst) as i64;
        let id = if unstable.load(Ordering::SeqCst) > 0 {
            serial + 1
        } else {
            100
        };
        ok(json!({"items":[row(id, now)],"total":1}))
    })
    .await;
    let e = engine();
    let rt = seed(&e, account(site, "split-status"), session(false));
    e.refresh_account("split-status", true).await.unwrap();
    let first = e.bootstrap().await.snapshot.unwrap();
    assert!(first.sync.complete && first.sync.usage.complete);
    assert_eq!(first.sync.usage.state, "synced");
    assert_eq!(first.sync.recent.state, "incomplete");
    assert!(!first.sync.recent.complete);
    assert_eq!(first.sync.state, "partial");
    assert_eq!(first.totals.unwrap().requests, 42);
    changing.store(0, Ordering::SeqCst);
    *rt.last_recent.lock().unwrap() = 0;
    e.refresh_account("split-status", false).await.unwrap();
    let recovered = e.bootstrap().await.snapshot.unwrap();
    assert_eq!(recovered.sync.state, "synced");
    assert!(recovered.sync.recent.complete);
    assert_eq!(summary_calls.load(Ordering::SeqCst), 1);
    task.abort();
    cleanup(&e);
}

#[tokio::test]
async fn balance_failure_keeps_usage_complete_and_last_balance_timestamp() {
    let failing = Arc::new(AtomicUsize::new(0));
    let observed = failing.clone();
    let (site, task) = server(move |request| {
        if request.starts_with("GET /api/v1/user/profile") {
            if observed.load(Ordering::SeqCst) > 0 {
                return (500, json!({"code":500}), vec![]);
            }
            return ok(json!({"balance":"10.25"}));
        }
        if request.starts_with("GET /api/v1/usage/stats") {
            return ok(json!({"total_requests":0,"total_actual_cost":0,
                "total_input_tokens":0,"total_output_tokens":0,
                "total_cache_read_tokens":0,"total_cache_creation_tokens":0}));
        }
        ok(json!({"items":[],"total":0}))
    })
    .await;
    let e = engine();
    seed(&e, account(site, "balance-status"), session(false));
    e.refresh_account("balance-status", true).await.unwrap();
    let first = e.bootstrap().await.snapshot.unwrap();
    failing.store(1, Ordering::SeqCst);
    e.refresh_account("balance-status", true).await.unwrap();
    let result = e.bootstrap().await.snapshot.unwrap();
    assert_eq!(result.sync.usage.state, "synced");
    assert!(result.sync.complete);
    assert_eq!(result.sync.balance.state, "stale");
    assert_eq!(result.sync.balance.synced_at, first.sync.balance.synced_at);
    assert_eq!(result.balance, first.balance);
    assert_eq!(result.sync.recent.state, "synced");
    assert_eq!(result.sync.state, "partial");
    task.abort();
    cleanup(&e);
}

#[test]
fn legacy_sync_status_does_not_invent_per_source_completeness() {
    let status: SyncStatus = serde_json::from_value(json!({
        "state":"incomplete", "message":"older incomplete snapshot",
        "usageSyncedAt":"2026-10-01T00:00:00Z", "balanceSyncedAt":null,
        "recentSyncedAt":"2026-10-01T00:00:00Z", "complete":false
    }))
    .unwrap();
    assert!(!status.usage.complete && !status.recent.complete && !status.balance.complete);
    assert_eq!(status.usage.state, "loading");
    assert_eq!(status.state, "incomplete");
}

#[tokio::test]
async fn filter_options_only_exports_names_and_ids_and_caches_per_account() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let (site, task) = server(move |request| {
        observed.fetch_add(1, Ordering::SeqCst);
        assert!(!request.contains("/admin/"));
        if request.starts_with("GET /api/v1/keys?") {
            return ok(
                json!({"items":[{"id":1,"name":"Work","key":"fixture-private-key",
                "user_id":7,"group":{"api_key":"fixture-other-secret"}}],"total":1}),
            );
        }
        assert!(request.starts_with("GET /api/v1/usage/dashboard/models?"));
        let params = request_params(&request);
        assert!(params.contains_key("start_date") && params.contains_key("end_date"));
        ok(json!({"models":[{"model":"model-b"},{"model":"model-a"},{"model":"model-a"}]}))
    })
    .await;
    let e = engine();
    seed(&e, account(site, "options"), session(false));
    let query = FilterOptionsQuery {
        account_id: "options".into(),
        range: UsageRange::Recent,
        force: false,
    };
    let options = e.filter_options(query.clone()).await.unwrap();
    assert_eq!(options.models, vec!["model-a", "model-b"]);
    assert!(options.models_complete && options.keys_complete);
    assert_eq!(options.keys[0].id, 1);
    let value = serde_json::to_string(&options).unwrap();
    assert!(
        !value.contains("fixture-private")
            && !value.contains("fixture-other")
            && !value.contains("user_id")
    );
    e.filter_options(query).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    e.state.lock().unwrap().config.accounts[0]
        .summary
        .needs_login = true;
    assert!(e
        .filter_options(FilterOptionsQuery {
            account_id: "options".into(),
            range: UsageRange::Recent,
            force: false,
        })
        .await
        .is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    task.abort();
    cleanup(&e);
}

#[tokio::test]
async fn duplicate_key_options_are_not_reported_as_complete() {
    let (site, task) = server(|request| {
        if request.starts_with("GET /api/v1/keys?") {
            return ok(json!({"items":[{"id":1,"name":"a"},{"id":1,"name":"a"}],"total":2}));
        }
        ok(json!({"models":[]}))
    })
    .await;
    let e = engine();
    seed(&e, account(site, "duplicate-options"), session(false));
    let result = e
        .filter_options(FilterOptionsQuery {
            account_id: "duplicate-options".into(),
            range: UsageRange::Today,
            force: false,
        })
        .await
        .unwrap();
    assert_eq!(result.keys.len(), 1);
    assert!(!result.keys_complete);
    assert!(result.message.unwrap().contains("尚未完整"));
    task.abort();
    cleanup(&e);
}

#[tokio::test]
async fn selected_recent_range_is_incomplete_until_its_records_are_covered() {
    let changing = Arc::new(AtomicUsize::new(1));
    let observed = changing.clone();
    let now = Utc::now() - Duration::seconds(1);
    let (site, task) = server(move |request| {
        if request.starts_with("GET /api/v1/user/profile") {
            return ok(json!({"balance":10}));
        }
        assert!(!request.contains("/usage/stats"));
        let id = observed.fetch_add(1, Ordering::SeqCst) as i64;
        ok(json!({"items":[row(id, now)],"total":1}))
    })
    .await;
    let e = engine();
    let rt = seed(&e, account(site, "recent-range"), session(false));
    *rt.range.lock().unwrap() = UsageRange::Recent;
    e.refresh_account("recent-range", true).await.unwrap();
    let result = e.bootstrap().await.snapshot.unwrap();
    assert_eq!(result.sync.state, "incomplete");
    assert_eq!(result.sync.usage.state, "incomplete");
    assert_eq!(result.sync.recent.state, "incomplete");
    assert!(!result.sync.complete);
    assert!(result.sync.balance.complete && result.sync.latest.complete);
    task.abort();
    cleanup(&e);
}

#[tokio::test]
async fn records_analysis_exposes_bucket_instants_and_key_identifiers() {
    let now = Utc::now() - Duration::seconds(1);
    let (site, task) = server(move |request| {
        assert!(request.starts_with("GET /api/v1/usage?"));
        ok(json!({"items":[row(1, now)],"total":1}))
    })
    .await;
    let e = engine();
    seed(&e, account(site, "record-buckets"), session(false));
    let result = e
        .analysis(AnalysisQuery {
            account_id: "record-buckets".into(),
            range: UsageRange::Recent,
            api_key_id: None,
            model: None,
            include_keys: true,
            force: false,
        })
        .await
        .unwrap();
    assert_eq!(result.trend_meta.source, "records");
    assert_eq!(result.trend_meta.timezone.as_deref(), Some("Asia/Shanghai"));
    assert_eq!(result.trend_meta.missing_buckets, "zero");
    assert!(result.trend_meta.complete);
    let bucket =
        DateTime::parse_from_rfc3339(result.trend[0].bucket_start.as_ref().unwrap()).unwrap();
    assert!(bucket <= now && now - bucket.with_timezone(&Utc) < Duration::minutes(1));
    assert!(result.keys[0].key_id.is_some());
    assert!(result.models[0].key_id.is_none());
    task.abort();
    cleanup(&e);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inflight_refresh_cannot_publish_a_previously_selected_range() {
    let started = Arc::new(AtomicUsize::new(0));
    let observed = started.clone();
    let (site, server) = server(move |request| {
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        if request.starts_with("GET /api/v1/usage/stats") {
            return ok(json!({"total_requests":0,"total_actual_cost":0,
                "total_input_tokens":0,"total_output_tokens":0,
                "total_cache_read_tokens":0,"total_cache_creation_tokens":0}));
        }
        if request.starts_with("GET /api/v1/user/profile") {
            return ok(json!({"balance":10}));
        }
        ok(json!({"items":[],"total":0}))
    })
    .await;
    let engine = Arc::new(engine());
    let a = account(site, "a");
    let rt = seed(&engine, a, session(false));
    let refreshing = engine.clone();
    let refresh = tokio::spawn(async move { refreshing.refresh_account("a", true).await });
    for _ in 0..100 {
        if started.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(started.load(Ordering::SeqCst) > 0);
    *rt.range.lock().unwrap() = UsageRange::Month;
    engine.state.lock().unwrap().generation += 1;
    refresh.await.unwrap().unwrap();
    let state = engine.bootstrap().await;
    assert_eq!(state.selected_range, UsageRange::Month);
    assert!(state.snapshot.is_none());
    assert_eq!(*rt.last_summary.lock().unwrap(), 0);
    server.abort();
    cleanup(&engine);
}

fn request_params(request: &str) -> HashMap<String, String> {
    let target = request.split_whitespace().nth(1).unwrap();
    url::Url::parse(&format!("http://fixture{target}"))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect()
}
fn fixture_record_page(request: &str, all: &[Value]) -> Value {
    let params = request_params(request);
    let tz = stats::timezone(&params["timezone"]).unwrap();
    let mut rows = all
        .iter()
        .filter(|r| {
            let date = DateTime::parse_from_rfc3339(r["created_at"].as_str().unwrap())
                .unwrap()
                .with_timezone(&tz)
                .format("%Y-%m-%d")
                .to_string();
            date >= params["start_date"]
                && date <= params["end_date"]
                && params
                    .get("api_key_id")
                    .is_none_or(|k| r["api_key_id"].as_i64().unwrap().to_string() == *k)
                && params
                    .get("model")
                    .is_none_or(|m| r["model"].as_str().unwrap() == m)
        })
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b["created_at"]
            .as_str()
            .unwrap()
            .cmp(a["created_at"].as_str().unwrap())
    });
    let page = params["page"].parse::<usize>().unwrap();
    let size = params["page_size"].parse::<usize>().unwrap();
    let total = rows.len();
    let items = rows
        .into_iter()
        .skip((page - 1) * size)
        .take(size)
        .collect::<Vec<_>>();
    json!({"items":items,"total":total,"page":page,"page_size":size})
}
fn record_query(range: UsageRange, page: u32, size: u32) -> RecordQuery {
    RecordQuery {
        account_id: "a".into(),
        range,
        page,
        page_size: size,
        api_key_id: None,
        model: None,
    }
}

#[tokio::test]
async fn large_rolling_history_is_ranked_and_can_page_past_five_thousand() {
    let now = Utc::now();
    let rows = (1..=15_000)
        .map(|i| row(i, now - Duration::seconds(i * 30)))
        .collect::<Vec<_>>();
    let calls = Arc::new(AtomicUsize::new(0));
    let downloaded = Arc::new(AtomicUsize::new(0));
    let (count, bytes) = (calls.clone(), downloaded.clone());
    let (site, server) = server(move |request| {
        count.fetch_add(1, Ordering::SeqCst);
        let data = fixture_record_page(&request, &rows);
        bytes.fetch_add(data["items"].as_array().unwrap().len(), Ordering::SeqCst);
        ok(data)
    })
    .await;
    let engine = engine();
    seed(&engine, account(site, "a"), session(false));
    let first = engine
        .query_records(record_query(UsageRange::Week, 1, 5))
        .await
        .unwrap();
    assert!(first.complete);
    assert_eq!(first.total, 15_000);
    assert_eq!(
        first.items.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert!(calls.load(Ordering::SeqCst) <= 32);
    assert!(downloaded.load(Ordering::SeqCst) <= 230);
    let before = calls.load(Ordering::SeqCst);
    let later = engine
        .query_records(record_query(UsageRange::Week, 1301, 5))
        .await
        .unwrap();
    assert!(later.complete);
    assert_eq!(later.total, 15_000);
    assert_eq!(
        later.items.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![6501, 6502, 6503, 6504, 6505]
    );
    assert_eq!(later.start, first.start);
    assert_eq!(later.end, first.end);
    assert!(calls.load(Ordering::SeqCst) - before <= 4);
    server.abort();
    cleanup(&engine);
}

#[tokio::test]
async fn rolling_record_boundaries_and_filters_are_exact() {
    let now = DateTime::parse_from_rfc3339("2026-10-02T00:02:00+08:00")
        .unwrap()
        .with_timezone(&Utc);
    for range in [UsageRange::Recent, UsageRange::Week] {
        let start = now
            - if range == UsageRange::Recent {
                Duration::minutes(5)
            } else {
                Duration::hours(168)
            };
        let mut rows = vec![];
        for (id, time) in [
            (1, now + Duration::seconds(1)),
            (2, now),
            (3, now - Duration::seconds(1)),
            (4, start),
            (5, start - Duration::seconds(1)),
        ] {
            rows.push(row(id, time));
        }
        for i in 0..1200 {
            let mut value = row(100 + i, start + Duration::seconds(1 + i / 10));
            if i % 2 == 0 {
                value["api_key_id"] = json!(2);
                value["model"] = json!("wanted");
            }
            rows.push(value);
        }
        let (site, server) = server(move |request| ok(fixture_record_page(&request, &rows))).await;
        let engine = engine();
        let a = account(site, "a");
        let rt = seed(&engine, a.clone(), session(false));
        let query = record_query(range, 1, 100);
        let result = engine
            .window_records(&a, &rt, &query, 1, 100, now)
            .await
            .unwrap();
        assert!(result.complete);
        assert_eq!(result.total, 1202);
        assert!(result.items.iter().all(|r| stats::within(r, start, now)));
        let filtered = RecordQuery {
            api_key_id: Some(2),
            model: Some("wanted".into()),
            ..query
        };
        let first = engine
            .window_records(&a, &rt, &filtered, 1, 100, now)
            .await
            .unwrap();
        assert!(first.complete);
        assert_eq!(first.total, 600);
        let last = engine
            .window_records(&a, &rt, &filtered, 6, 100, now + Duration::seconds(1))
            .await
            .unwrap();
        assert!(last.complete);
        assert_eq!(last.items.len(), 100);
        assert_eq!(last.start, start);
        assert_eq!(last.end, now);
        assert!(last
            .items
            .iter()
            .all(|r| stats::within(r, start, now) && r.api_key_id == 2 && r.model == "wanted"));
        server.abort();
        cleanup(&engine);
    }
}

#[tokio::test]
async fn changed_pagination_never_returns_a_final_rolling_page() {
    let now = Utc::now();
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let (site, server) = server(move |request| {
        let call = observed.fetch_add(1, Ordering::SeqCst) as i64;
        let rows = (1..=200)
            .map(|i| row(i + call * 1000, now - Duration::seconds(i)))
            .collect::<Vec<_>>();
        ok(fixture_record_page(&request, &rows))
    })
    .await;
    let engine = engine();
    seed(&engine, account(site, "a"), session(false));
    let result = engine
        .query_records(record_query(UsageRange::Recent, 1, 20))
        .await
        .unwrap();
    assert!(!result.complete);
    assert_eq!(result.total, 0);
    assert!(result.items.is_empty());
    assert!(result.message.unwrap().contains("发生变化"));
    server.abort();
    cleanup(&engine);
}

#[tokio::test]
async fn rolling_page_context_expires_and_server_page_metadata_is_checked() {
    let now = Utc::now();
    let rows = (1..=200)
        .map(|i| row(i, now - Duration::seconds(i)))
        .collect::<Vec<_>>();
    let (site, server) = server(move |request| ok(fixture_record_page(&request, &rows))).await;
    let engine = engine();
    let a = account(site, "a");
    let rt = seed(&engine, a.clone(), session(false));
    let query = record_query(UsageRange::Recent, 1, 20);
    assert!(
        engine
            .window_records(&a, &rt, &query, 1, 20, now)
            .await
            .unwrap()
            .complete
    );
    let expired = engine
        .window_records(&a, &rt, &query, 2, 20, now + Duration::seconds(60))
        .await
        .unwrap();
    assert!(!expired.complete);
    assert!(expired.message.unwrap().contains("过期"));
    server.abort();
    cleanup(&engine);
}

#[tokio::test]
async fn rolling_record_ranking_rejects_mismatched_server_pagination() {
    let (site, server) =
        server(|_| ok(json!({"items":[],"total":0,"page":1,"page_size":20}))).await;
    let engine = engine();
    seed(&engine, account(site, "a"), session(false));
    let error = engine
        .query_records(record_query(UsageRange::Recent, 1, 20))
        .await
        .err()
        .unwrap();
    assert!(error.contains("分页参数"));
    server.abort();
    cleanup(&engine);
}
