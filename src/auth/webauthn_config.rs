use anyhow::Result;
use webauthn_rs::prelude::*;

/// Build a Webauthn instance from the server address.
/// For localhost, uses http://; otherwise https://.
pub fn build_webauthn(addr: &str) -> Result<Webauthn> {
    let host = addr.split(':').next().unwrap_or("localhost");
    let port: u16 = addr
        .split(':')
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(6969);

    let (rp_id, rp_origin) = if matches!(host, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0") {
        (
            "localhost".to_string(),
            format!("http://localhost:{}", port),
        )
    } else {
        (host.to_string(), format!("https://{}", addr))
    };

    let rp_origin = Url::parse(&rp_origin)?;

    let builder = WebauthnBuilder::new(&rp_id, &rp_origin)?.rp_name("abot");

    Ok(builder.build()?)
}
