//! 共用的 HTTP 用戶端。
//!
//! TLS 用 rustls（密碼學用 ring）加內含的根憑證（webpki-roots）：不依賴系統的憑證庫，
//! Android 與電腦上行為一致，也不需要 Java 端配合。

use std::sync::Arc;
use std::time::Duration;

pub fn build_client() -> reqwest::Client {
    builder().build().expect("static client configuration")
}

/// 逾時照指定的用戶端（快速翻譯用：連線與整個請求都比全文翻譯短得多）。
pub fn build_client_with(connect: Duration, total: Duration) -> reqwest::Client {
    builder()
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .expect("static client configuration")
}

/// 設好 TLS 與逾時的 builder，給要另外加限制的用途（應用內更新的轉址與 `Referer`）。
pub fn builder() -> reqwest::ClientBuilder {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .connect_timeout(Duration::from_secs(15))
        // 一批翻譯可能要等模型寫很久
        .timeout(Duration::from_secs(180))
}
