//! Windows 的金鑰與剪貼簿，直接呼叫系統介面（`windows-sys`）。
//!
//! 金鑰存在 Windows 憑證管理員（一般認證，目標名稱 `PaperReader:<名稱>`，只存在這台電腦、不漫遊）。
//! 不用 `keyring` 套件：它不能列舉，而畫面要知道哪幾個服務已設金鑰（`secret_names`）；
//! 憑證管理員的列舉可以用前綴篩選，只會讀到這個應用自己的項目。`windows-sys` 本來就在相依圖裡。
//! 錯誤訊息只帶系統的錯誤碼，不帶名稱以外的內容，金鑰不會進日誌。

use crate::{Error, Result};
use std::ffi::c_void;
use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, GetLastError, GlobalFree};
use windows_sys::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
    CredDeleteW, CredEnumerateW, CredFree, CredReadW, CredWriteW,
};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

/// 憑證管理員裡這個應用的項目的前綴。
pub(crate) const SECRET_PREFIX: &str = "PaperReader:";

/// 以 0 結尾的 UTF-16（系統介面的字串）。
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 系統介面失敗：帶上哪一步與錯誤碼。
fn last_error(step: &str) -> Error {
    // SAFETY: 只讀這個執行緒最後一個錯誤碼
    let code = unsafe { GetLastError() };
    Error::Platform(format!("{step} failed: {code}"))
}

/// 名稱只能是小寫英數字與底線，1 到 40 個字（同 Android）。
fn valid_name(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn target(prefix: &str, name: &str) -> Result<Vec<u16>> {
    if valid_name(name) {
        Ok(wide(&format!("{prefix}{name}")))
    } else {
        Err(Error::Platform("invalid secret".into()))
    }
}

pub(crate) fn secret_set(prefix: &str, name: &str, value: &str) -> Result<()> {
    let mut target = target(prefix, name)?;
    let blob = value.as_bytes();
    if blob.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
        return Err(Error::Platform("secret too long".into()));
    }
    let mut user = wide("PaperReader");
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: user.as_mut_ptr(),
        ..Default::default()
    };
    // SAFETY: 各個指標在呼叫期間都有效；系統只讀不寫
    if unsafe { CredWriteW(&cred, 0) } == 0 {
        return Err(last_error("CredWriteW"));
    }
    Ok(())
}

pub(crate) fn secret_get(prefix: &str, name: &str) -> Result<Option<String>> {
    let target = target(prefix, name)?;
    let mut cred: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: 成功時系統配置 `cred`，下面用完以 `CredFree` 還回去
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut cred) } == 0 {
        // SAFETY: 同上
        if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            return Ok(None);
        }
        return Err(last_error("CredReadW"));
    }
    // SAFETY: `cred` 由系統配置、內容有效到 `CredFree` 為止
    let value = unsafe {
        let c = &*cred;
        let bytes = if c.CredentialBlob.is_null() || c.CredentialBlobSize == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
        };
        CredFree(cred as *const c_void);
        bytes
    };
    String::from_utf8(value)
        .map(Some)
        .map_err(|_| Error::Platform("the stored secret is not text".into()))
}

pub(crate) fn secret_delete(prefix: &str, name: &str) -> Result<()> {
    let target = target(prefix, name)?;
    // SAFETY: 目標名稱在呼叫期間有效
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        // SAFETY: 同上
        if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            return Ok(());
        }
        return Err(last_error("CredDeleteW"));
    }
    Ok(())
}

/// 已設金鑰的名稱：只列目標名稱以 `prefix` 開頭的項目，不讀內容。
pub(crate) fn secret_names(prefix: &str) -> Result<Vec<String>> {
    let filter = wide(&format!("{prefix}*"));
    let mut count: u32 = 0;
    let mut creds: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: 成功時系統配置 `creds`，下面用完以 `CredFree` 還回去
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut creds) } == 0 {
        // SAFETY: 同上
        if unsafe { GetLastError() } == ERROR_NOT_FOUND {
            return Ok(Vec::new());
        }
        return Err(last_error("CredEnumerateW"));
    }
    let mut names = Vec::new();
    // SAFETY: `creds` 指向 `count` 個有效的項目，到 `CredFree` 為止
    unsafe {
        for i in 0..count as usize {
            let c = &**creds.add(i);
            if c.TargetName.is_null() {
                continue;
            }
            let mut len = 0;
            while *c.TargetName.add(len) != 0 {
                len += 1;
            }
            let full = String::from_utf16_lossy(std::slice::from_raw_parts(c.TargetName, len));
            if let Some(name) = full.strip_prefix(prefix)
                && valid_name(name)
            {
                names.push(name.to_string());
            }
        }
        CredFree(creds as *const c_void);
    }
    names.sort();
    Ok(names)
}

// ---------- 剪貼簿 ----------

/// Windows 剪貼簿的「HTML Format」：一段標頭寫明 HTML 與片段的位元組位置，後面是 UTF-8 的 HTML。
fn cf_html(fragment: &str) -> String {
    const PRE: &str = "<html><body>\r\n<!--StartFragment-->";
    const POST: &str = "<!--EndFragment-->\r\n</body></html>";
    let header = |a: usize, b: usize, c: usize, d: usize| {
        format!(
            "Version:0.9\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n"
        )
    };
    let start_html = header(0, 0, 0, 0).len();
    let start_fragment = start_html + PRE.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + POST.len();
    format!(
        "{}{PRE}{fragment}{POST}",
        header(start_html, end_html, start_fragment, end_fragment)
    )
}

/// 把一段位元組交給剪貼簿（剪貼簿已開、已清空）。成功之後那塊記憶體歸系統管。
fn set_data(format: u32, bytes: &[u8]) -> Result<()> {
    // SAFETY: 配置、鎖定、複製、解鎖；交給剪貼簿失敗時自己釋放
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
        if h.is_null() {
            return Err(last_error("GlobalAlloc"));
        }
        let p = GlobalLock(h);
        if p.is_null() {
            GlobalFree(h);
            return Err(last_error("GlobalLock"));
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.cast::<u8>(), bytes.len());
        GlobalUnlock(h);
        if SetClipboardData(format, h).is_null() {
            GlobalFree(h);
            return Err(last_error("SetClipboardData"));
        }
    }
    Ok(())
}

/// 放一段文字到剪貼簿；有 `html` 時一併放 HTML（貼到文書軟體保留格式）。`owner` 是應用的視窗。
pub(crate) fn copy_text(owner: Option<*mut c_void>, text: &str, html: Option<&str>) -> Result<()> {
    let owner = owner.unwrap_or(std::ptr::null_mut());
    // 別的程式正開著剪貼簿時稍等再試
    let mut opened = false;
    for _ in 0..10 {
        // SAFETY: 視窗代號可以是空的
        if unsafe { OpenClipboard(owner) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !opened {
        return Err(last_error("OpenClipboard"));
    }
    let result = (|| {
        // SAFETY: 剪貼簿已由這個執行緒開著
        if unsafe { EmptyClipboard() } == 0 {
            return Err(last_error("EmptyClipboard"));
        }
        let utf16: Vec<u8> = wide(text).iter().flat_map(|u| u.to_le_bytes()).collect();
        set_data(u32::from(CF_UNICODETEXT), &utf16)?;
        if let Some(html) = html {
            let name = wide("HTML Format");
            // SAFETY: 名稱在呼叫期間有效
            let format = unsafe { RegisterClipboardFormatW(name.as_ptr()) };
            if format == 0 {
                return Err(last_error("RegisterClipboardFormatW"));
            }
            let mut bytes = cf_html(html).into_bytes();
            bytes.push(0);
            set_data(format, &bytes)?;
        }
        Ok(())
    })();
    // SAFETY: 上面開的剪貼簿
    unsafe { CloseClipboard() };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_names_follow_the_android_rule() {
        assert!(valid_name("deepl"));
        assert!(valid_name("openai_2"));
        assert!(!valid_name(""));
        assert!(!valid_name("DeepL"));
        assert!(!valid_name("a-b"));
        assert!(!valid_name("a:b"));
        assert!(!valid_name(&"a".repeat(41)));
        assert!(target(SECRET_PREFIX, "x*").is_err());
    }

    #[test]
    fn html_on_the_clipboard_carries_correct_offsets() {
        let frag = "<i>斜體</i> text";
        let s = cf_html(frag);
        let num = |key: &str| -> usize {
            let at = s.find(key).unwrap() + key.len();
            s[at..at + 10].parse().unwrap()
        };
        let (sh, eh, sf, ef) = (
            num("StartHTML:"),
            num("EndHTML:"),
            num("StartFragment:"),
            num("EndFragment:"),
        );
        assert_eq!(&s[sf..ef], frag);
        assert!(s[sh..].starts_with("<html>"));
        assert_eq!(eh, s.len());
        assert!(s[..sh].starts_with("Version:0.9"));
    }

    /// 真的寫進這台電腦的憑證管理員（項目名稱 `PaperReaderSelfTest:`，測完刪掉）。
    /// 會動到系統的憑證儲存，平常不跑：`cargo test -p tauri-plugin-paperreader -- --ignored`。
    #[test]
    #[ignore = "writes to the Windows Credential Manager"]
    fn secrets_round_trip_through_the_credential_manager() {
        const PREFIX: &str = "PaperReaderSelfTest:";
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = secret_delete(PREFIX, "fake_one");
                let _ = secret_delete(PREFIX, "fake_two");
            }
        }
        let _cleanup = Cleanup;
        assert_eq!(secret_get(PREFIX, "fake_one").unwrap(), None);
        secret_set(PREFIX, "fake_one", "not-a-real-key-1").unwrap();
        secret_set(PREFIX, "fake_two", "假的金鑰 2").unwrap();
        assert_eq!(
            secret_get(PREFIX, "fake_one").unwrap().as_deref(),
            Some("not-a-real-key-1")
        );
        assert_eq!(
            secret_get(PREFIX, "fake_two").unwrap().as_deref(),
            Some("假的金鑰 2")
        );
        assert_eq!(secret_names(PREFIX).unwrap(), vec!["fake_one", "fake_two"]);
        secret_set(PREFIX, "fake_one", "changed").unwrap();
        assert_eq!(
            secret_get(PREFIX, "fake_one").unwrap().as_deref(),
            Some("changed")
        );
        secret_delete(PREFIX, "fake_one").unwrap();
        secret_delete(PREFIX, "fake_one").unwrap();
        assert_eq!(secret_get(PREFIX, "fake_one").unwrap(), None);
        assert_eq!(secret_names(PREFIX).unwrap(), vec!["fake_two"]);
    }
}
