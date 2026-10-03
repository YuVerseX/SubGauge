use super::*;

const PAYLOAD: &[u8] = b"SubGauge isolated signing regression\n";
const FIXTURE_PUBLIC: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDJGMDk3NjAyQUMxOTAxQ0MKUldUTUFSbXNBbllKTHc2TStMbGdvOUswR0pWTC9wakl0UXg5TkhjRk1jY3B4Si8vUWtDL2IvbXUK";
const FIXTURE_SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUTUFSbXNBbllKTDgwd2w1S1h0S1VnRW51RGhpWWpVSmhlTzhQVTMxblovZHFrcEdBVDBzUTFnVFhZR0FEbXZDYmdhclRiYXhtdVQ3ZlhuWE04SzY1QzU1LzJkKzlvUFFvPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxMDEzNTk3CWZpbGU6Zml4dHVyZS50eHQJdmVyc2lvbjo5LjkuOQp6RjNreXMrSWVqOUNqeDNhU1h5RUJoZEdMSDdCVm5FbEkwM2pKWEhuRVdkaEdGMGJYQzdmeWdOOXMxUHREUEwzbjBLM2k4UCsyRTNFdW1GNFhSbjdBdz09Cg==";

#[test]
fn signature_binds_exact_bytes_global_comment_and_announced_version() {
    assert!(verify_package(PAYLOAD, FIXTURE_SIGNATURE, FIXTURE_PUBLIC, "9.9.9").is_ok());
    assert!(verify_package(PAYLOAD, FIXTURE_SIGNATURE, FIXTURE_PUBLIC, "v9.9.9").is_ok());
    assert!(verify_package(PAYLOAD, FIXTURE_SIGNATURE, FIXTURE_PUBLIC, "9.9.10").is_err());
    assert!(verify_package(b"tampered", FIXTURE_SIGNATURE, FIXTURE_PUBLIC, "9.9.9").is_err());
    assert!(verify_package(PAYLOAD, FIXTURE_SIGNATURE, PUBLIC_KEY.trim(), "9.9.9").is_err());
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(FIXTURE_SIGNATURE)
        .unwrap();
    let forged = String::from_utf8(decoded)
        .unwrap()
        .replace("version:9.9.9", "version:9.9.10");
    let forged = base64::engine::general_purpose::STANDARD.encode(forged);
    assert!(verify_package(PAYLOAD, &forged, FIXTURE_PUBLIC, "9.9.10").is_err());
    assert!(verify_package(&[], FIXTURE_SIGNATURE, FIXTURE_PUBLIC, "9.9.9").is_err());
}

#[test]
fn release_scope_rejects_cross_repository_credentials_ports_and_version_mismatch() {
    let valid = "https://github.com/YuVerseX/SubGauge/releases/download/v0.1.9/SubGauge_0.1.9_x64-setup.exe";
    assert!(validate_release("0.1.9", &Url::parse(valid).unwrap()).is_ok());
    let build = "https://github.com/YuVerseX/SubGauge/releases/download/v0.1.9%2Bbuild.1/SubGauge_0.1.9%2Bbuild.1_x64-setup.exe";
    assert!(validate_release("0.1.9+build.1", &Url::parse(build).unwrap()).is_ok());
    assert!(validate_release("0.1.9+build.2", &Url::parse(build).unwrap()).is_err());
    assert!(parse_version("vv0.1.9").is_err());
    assert!(validate_release("0.1.10", &Url::parse(valid).unwrap()).is_err());
    for value in [
        valid.replace("https:", "http:"),
        valid.replace("YuVerseX", "another"),
        valid.replace("SubGauge/releases", "Other/releases"),
        valid.replace("github.com", "github.com.example.com"),
        valid.replace("github.com", "user@github.com"),
        valid.replace("github.com", "github.com:8443"),
        format!("{valid}?redirect=1"),
        format!("{valid}#fragment"),
        valid.replace("x64-setup.exe", "windows-x64.zip"),
    ] {
        assert!(validate_release("0.1.9", &Url::parse(&value).unwrap()).is_err());
    }
    assert!(!allowed_download_host(
        &Url::parse("https://localhost/file").unwrap()
    ));
    assert!(!allowed_download_host(
        &Url::parse("http://release-assets.githubusercontent.com/file").unwrap()
    ));
    assert!(allowed_download_host(
        &Url::parse("https://release-assets.githubusercontent.com/file?temporary=1").unwrap()
    ));
}

#[test]
fn background_check_is_daily_and_recovers_from_a_clock_rollback() {
    assert!(check_due(None, 100));
    assert!(!check_due(Some(100), 101));
    assert!(!check_due(Some(100), 100 + CHECK_SECONDS - 1));
    assert!(check_due(Some(100), 100 + CHECK_SECONDS));
    assert!(check_due(Some(101), 100));
}

#[test]
fn preference_patch_distinguishes_omitted_skip_from_explicit_clear() {
    let absent: PreferencesPatch =
        serde_json::from_value(serde_json::json!({"autoCheck":false})).unwrap();
    assert_eq!(absent.auto_check, Some(false));
    assert_eq!(absent.skip_version, None);
    let clear: PreferencesPatch =
        serde_json::from_value(serde_json::json!({"skipVersion":null})).unwrap();
    assert_eq!(clear.skip_version, Some(None));
    let skip: PreferencesPatch =
        serde_json::from_value(serde_json::json!({"skipVersion":"0.1.9"})).unwrap();
    assert_eq!(skip.skip_version, Some(Some("0.1.9".into())));
    assert!(serde_json::from_value::<PreferencesPatch>(
        serde_json::json!({"endpoint":"http://localhost"})
    )
    .is_err());
}

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "subgauge-update-regression-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn service(&self) -> Updates {
        Updates::new(
            self.0.join("updates.v1.json"),
            "0.1.8".into(),
            Distribution::Installed,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn corrupt_or_newer_preferences_are_preserved_and_do_not_enable_background_checks() {
    let fixture = Fixture::new();
    let path = fixture.0.join("updates.v1.json");
    for bytes in [
        b"{broken}".as_slice(),
        br#"{"version":2,"autoCheck":true,"skippedVersion":null,"lastCheck":null,"attempt":null}"#
            .as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let service = fixture.service();
        assert!(!service.status().auto_check);
        assert!(service.status().error.is_some());
        assert!(!service.due(Utc::now().timestamp()));
        let mut inner = service.inner.lock().unwrap();
        assert!(service
            .save_preferences(&mut inner, Preferences::default())
            .is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn preferences_persist_and_failed_save_does_not_change_successful_state() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let next = Preferences {
        auto_check: false,
        skipped_version: Some("0.1.9".into()),
        ..Default::default()
    };
    service
        .save_preferences(&mut service.inner.lock().unwrap(), next.clone())
        .unwrap();
    let reloaded = fixture.service();
    assert!(!reloaded.status().auto_check);
    assert_eq!(reloaded.status().skipped_version.as_deref(), Some("0.1.9"));
    let blocker = fixture.0.join("blocked");
    std::fs::write(&blocker, b"file").unwrap();
    let broken = Updates::new(
        blocker.join("updates.v1.json"),
        "0.1.8".into(),
        Distribution::Installed,
    );
    let mut inner = broken.inner.lock().unwrap();
    let original = inner.preferences.auto_check;
    assert!(broken.save_preferences(&mut inner, next).is_err());
    assert_eq!(inner.preferences.auto_check, original);
}

#[test]
fn restart_detects_an_unfinished_install_without_promising_automatic_rollback() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let next = Preferences {
        attempt: Some(Attempt {
            version: "0.1.9".into(),
        }),
        ..Default::default()
    };
    service
        .save_preferences(&mut service.inner.lock().unwrap(), next)
        .unwrap();
    assert!(fixture
        .service()
        .status()
        .error
        .unwrap()
        .contains("当前仍为"));
    let upgraded = Updates::new(
        service.path.clone(),
        "0.1.9".into(),
        Distribution::Installed,
    );
    assert!(upgraded.status().error.is_none());
    assert!(upgraded.inner.lock().unwrap().preferences.attempt.is_none());
}

#[tokio::test]
async fn install_barrier_rejects_new_operations_and_waits_for_existing_activity() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let current = service.activity().await.unwrap();
    service.installing.store(true, Ordering::Release);
    assert!(service.activity().await.is_err());
    assert!(service.activity.clone().try_write_owned().is_err());
    drop(current);
    let exclusive = service.activity.clone().try_write_owned().unwrap();
    drop(exclusive);
    service.installing.store(false, Ordering::Release);
    assert!(service.activity().await.is_ok());
}

#[test]
fn stale_revision_or_wrong_phase_cannot_download_or_install() {
    let fixture = Fixture::new();
    let mut view = fixture.service().status();
    view.phase = Phase::Ready;
    view.revision = 3;
    assert!(validate_action(&view, 3, &[Phase::Ready]).is_ok());
    assert!(validate_action(&view, 2, &[Phase::Ready]).is_err());
    assert!(validate_action(&view, 3, &[Phase::Available]).is_err());
}

#[cfg(windows)]
#[test]
fn custom_install_directory_with_spaces_is_the_final_unquoted_nsis_argument() {
    assert_eq!(
        installer_directory_argument(std::path::Path::new(
            r"D:\Program Files\SubGauge\subgauge.exe"
        ))
        .unwrap(),
        r"/D=D:\Program Files\SubGauge"
    );
    assert!(installer_directory_argument(std::path::Path::new("subgauge.exe")).is_err());
    assert!(
        installer_directory_argument(std::path::Path::new("D:\\Bad\nPath\\subgauge.exe")).is_err()
    );
}

#[cfg(windows)]
#[test]
#[ignore = "requires Windows WebView2; isolated hidden app and local HTTP fixtures only"]
fn native_updater_smoke() {
    use std::io::{Read, Write};
    use tauri::Manager;
    let fixture = Fixture::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/metadata",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let tampered = Arc::new(AtomicBool::new(false));
    let wrong_version = Arc::new(AtomicBool::new(false));
    let server = {
        let stopped = stopped.clone();
        let tampered = tampered.clone();
        let wrong_version = wrong_version.clone();
        std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = [0u8; 4096];
                let count = stream.read(&mut request).unwrap_or(0);
                if String::from_utf8_lossy(&request[..count]).starts_with("GET /package ") {
                    let payload = if tampered.load(Ordering::Acquire) {
                        b"tampered".as_slice()
                    } else {
                        PAYLOAD
                    };
                    // No Content-Length: exercise real streaming and unknown-length progress.
                    write!(stream, "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n", payload.len()).unwrap();
                    stream.write_all(payload).unwrap();
                    stream.write_all(b"\r\n0\r\n\r\n").unwrap();
                } else {
                    let version = if wrong_version.load(Ordering::Acquire) {
                        "9.9.10"
                    } else {
                        "9.9.9"
                    };
                    let body = serde_json::to_vec(&serde_json::json!({
                        "version":version, "notes":"Isolated signed fixture", "pub_date":"2026-10-03T01:00:00Z",
                        "platforms":{"windows-x86_64":{"url":format!("https://github.com/YuVerseX/SubGauge/releases/download/v{version}/SubGauge_{version}_x64-setup.exe"),"signature":FIXTURE_SIGNATURE}}
                    })).unwrap();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                    stream.write_all(&body).unwrap();
                }
            }
        })
    };
    let outcome = Arc::new(Mutex::new(None::<Result<(), String>>));
    let target_outcome = outcome.clone();
    let data = fixture.0.clone();
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = format!("app.subgauge.update-smoke-{}", uuid::Uuid::new_v4());
    context.config_mut().plugins.0.insert("updater".into(), serde_json::json!({
        "pubkey":FIXTURE_PUBLIC,"requireSignedVersion":true,"dangerousInsecureTransportProtocol":true,
        "endpoints":[endpoint.as_str()],"windows":{"installMode":"passive"}
    }));
    let app = tauri::Builder::default()
        .any_thread()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let hidden = tauri::WebviewWindowBuilder::new(
                app,
                "float",
                tauri::WebviewUrl::External("about:blank".parse().unwrap()),
            )
            .visible(false)
            .data_directory(data.join("webview"))
            .build()?;
            app.manage(crate::core::Engine::new(data.clone()).map_err(std::io::Error::other)?);
            app.manage(crate::windows::Windows::new(data.join("window.json")));
            let mut updates = Updates::new(
                data.join("updates.v1.json"),
                "0.1.8".into(),
                Distribution::Installed,
            );
            updates.test_transport = Some(TestTransport {
                endpoint,
                public_key: FIXTURE_PUBLIC,
            });
            app.manage(updates);
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result = async {
                    let service = handle.state::<Updates>();
                    let available = service.check(&handle, true, false).await;
                    if available.phase != Phase::Available
                        || available.version.as_deref() != Some("9.9.9")
                    {
                        return Err("Native check failed".into());
                    }
                    if available.published_at.as_deref() != Some("2026-10-03T01:00:00+00:00") {
                        return Err("Publication date is not RFC3339".into());
                    }
                    if service
                        .download(&handle, available.revision - 1)
                        .await
                        .is_ok()
                    {
                        return Err("Stale request was accepted".into());
                    }
                    let ready = service.download(&handle, available.revision).await?;
                    if ready.phase != Phase::Ready
                        || ready.total_bytes.is_some()
                        || ready.downloaded_bytes != PAYLOAD.len() as u64
                    {
                        return Err("Native signed unknown-length download failed".into());
                    }
                    let retained = service.check(&handle, true, true).await;
                    if retained.revision != ready.revision
                        || !service.inner.lock().unwrap().bytes.is_some()
                    {
                        return Err("Tray check discarded a verified download".into());
                    }
                    // Fail preparation before Update.install(): the signed fixture is text, never executable.
                    std::fs::create_dir(data.join("accounts.v1.json"))
                        .map_err(|_| "Could not prepare save failure")?;
                    if service.install(&handle, retained.revision).await.is_ok() {
                        return Err("Install continued after account save failure".into());
                    }
                    let failed = service.status();
                    if failed.phase != Phase::Ready
                        || failed.error.is_none()
                        || service.is_installing()
                        || service.activity().await.is_err()
                    {
                        return Err("Failed preparation did not restore usable state".into());
                    }
                    tampered.store(true, Ordering::Release);
                    let next = service.check(&handle, true, false).await;
                    let rejected = service.download(&handle, next.revision).await?;
                    if rejected.phase != Phase::Error
                        || service.inner.lock().unwrap().bytes.is_some()
                    {
                        return Err("Tampered download reached ready".into());
                    }
                    tampered.store(false, Ordering::Release);
                    wrong_version.store(true, Ordering::Release);
                    let next = service.check(&handle, true, false).await;
                    let rejected = service.download(&handle, next.revision).await?;
                    if rejected.phase != Phase::Error
                        || !rejected
                            .error
                            .as_deref()
                            .is_some_and(|v| v.contains("版本"))
                    {
                        return Err("Signed version mismatch was not rejected".into());
                    }
                    service.inner.lock().unwrap().view.distribution = Distribution::Portable;
                    if service.download(&handle, rejected.revision).await.is_ok() {
                        return Err("Portable distribution could install".into());
                    }
                    Ok(())
                }
                .await;
                *target_outcome.lock().unwrap() = Some(result);
                let _ = crate::windows::on_ui(handle.clone(), move |app| {
                    hidden.destroy().map_err(|_| "Window cleanup failed")?;
                    app.exit(0);
                    Ok(())
                })
                .await;
            });
            Ok(())
        })
        .build(context)
        .unwrap();
    app.run_return(|_, _| {});
    stopped.store(true, Ordering::Release);
    server.join().unwrap();
    let result = outcome
        .lock()
        .unwrap()
        .take()
        .expect("Native update task did not finish");
    result.unwrap();
    // WebView2 profile teardown may finish shortly after the HWND is destroyed.
    for _ in 0..10 {
        if std::fs::remove_dir_all(&fixture.0).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        !fixture.0.exists(),
        "Native fixture profile was not released"
    );
    std::mem::forget(fixture);
}
