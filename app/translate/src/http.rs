//! 共用的 HTTP 用戶端。
//!
//! TLS 用 rustls（密碼學用 ring）加內含的根憑證（webpki-roots）：不依賴系統的憑證庫，
//! Android 與電腦上行為一致，也不需要 Java 端配合。

use std::sync::Arc;
use std::time::Duration;

pub fn build_client() -> reqwest::Client {
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
        .build()
        .expect("static client configuration")
}
