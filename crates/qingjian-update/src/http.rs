//! 桌面使用系统证书；安卓使用 ring 与内置根证书，不依赖 Java TLS 初始化。

#[cfg(feature = "native-tls")]
fn base() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
}

#[cfg(all(feature = "rustls", not(feature = "native-tls")))]
fn base() -> reqwest::ClientBuilder {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports TLS")
    .with_root_certificates(roots)
    .with_no_client_auth();
    reqwest::Client::builder().tls_backend_preconfigured(tls)
}

pub(crate) fn builder(proxy: &str) -> Result<reqwest::ClientBuilder, crate::UpdateError> {
    let mut client = base().https_only(true);
    if !proxy.trim().is_empty() {
        client = client.proxy(reqwest::Proxy::all(proxy.trim())?);
    }
    Ok(client)
}
