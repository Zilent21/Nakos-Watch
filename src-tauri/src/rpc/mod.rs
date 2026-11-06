pub mod core_rpc;
pub mod electrum;
pub mod explorer;
pub mod fees;

/// Install `ring` as rustls' process-level crypto provider, once. Without this, any TLS
/// client built through rustls' default builder panics ("Could not automatically determine
/// the process-level CryptoProvider"), which aborts the app. Safe to call repeatedly.
pub fn ensure_tls_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
