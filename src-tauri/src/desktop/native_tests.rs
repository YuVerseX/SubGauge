#![cfg(windows)]

use super::*;
use windows_sys::Win32::System::Registry::*;

struct RegistryFixture {
    path: String,
}

impl RegistryFixture {
    fn new() -> Self {
        Self {
            path: format!("Software\\SubGaugeTests\\Desktop\\{}", uuid::Uuid::new_v4()),
        }
    }
    fn raw_value(&self, name: &str, kind: u32, bytes: &[u8]) {
        assert!(self.path.starts_with("Software\\SubGaugeTests\\Desktop\\"));
        let mut key = std::ptr::null_mut();
        let result = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                wide(&self.path).as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        assert_eq!(result, 0);
        let key = Registry(key);
        assert_eq!(
            unsafe {
                RegSetValueExW(
                    key.0,
                    wide(name).as_ptr(),
                    0,
                    kind,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            },
            0
        );
    }
}

impl Drop for RegistryFixture {
    fn drop(&mut self) {
        // Only this test's newly generated GUID subtree is eligible for cleanup.
        assert!(self.path.starts_with("Software\\SubGaugeTests\\Desktop\\"));
        let _ = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wide(&self.path).as_ptr()) };
    }
}

#[test]
fn isolated_registry_roundtrip_preserves_spaces_unicode_and_other_values() {
    let fixture = RegistryFixture::new();
    let command = r#""D:\Program Files\中文\subgauge.exe" --autostart"#;
    assert_eq!(read_run_at(&fixture.path, "fixture"), Ok(None));
    assert!(write_run_at(&fixture.path, "fixture", None).is_ok());
    write_run_at(&fixture.path, "neighbor", Some("keep me")).unwrap();
    write_run_at(&fixture.path, "fixture", Some(command)).unwrap();
    assert_eq!(
        read_run_at(&fixture.path, "fixture").unwrap().as_deref(),
        Some(command)
    );
    write_run_at(&fixture.path, "fixture", None).unwrap();
    assert_eq!(read_run_at(&fixture.path, "fixture"), Ok(None));
    assert_eq!(
        read_run_at(&fixture.path, "neighbor").unwrap().as_deref(),
        Some("keep me")
    );
    write_run_at(&fixture.path, "fixture", None).unwrap();
}

#[test]
fn isolated_registry_rejects_binary_expandable_or_malformed_strings_without_rewriting() {
    let fixture = RegistryFixture::new();
    for (kind, bytes) in [
        (REG_BINARY, vec![2, 0, 0, 0]),
        (REG_EXPAND_SZ, vec![b'x', 0, 0, 0]),
        (REG_SZ, vec![b'x']),
        (REG_SZ, vec![b'x', 0, 0, 0, b'y', 0, 0, 0]),
        (REG_SZ, vec![0, 0xd8, 0, 0]),
    ] {
        fixture.raw_value("fixture", kind, &bytes);
        assert!(read_run_at(&fixture.path, "fixture").is_err());
    }
    write_run_at(&fixture.path, "neighbor", Some("unchanged")).unwrap();
    assert_eq!(
        read_run_at(&fixture.path, "neighbor").unwrap().as_deref(),
        Some("unchanged")
    );
}

#[test]
fn isolated_registry_rejects_oversized_string_value() {
    let fixture = RegistryFixture::new();
    fixture.raw_value("fixture", REG_SZ, &vec![0u8; 65538]);
    assert!(read_run_at(&fixture.path, "fixture").is_err());
}
