//! 使用 OpenAI 公布的公钥验证登录身份与授权范围。

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::Deserialize;

use super::{AuthError, TokenResponse};

/// 验证后的账户身份。
pub struct Identity {
    pub subject: String,
    pub email: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    nonce: String,
    iat: u64,
    email: Option<String>,
}

/// 从固定的 OpenAI 地址取得公钥，验证签名、签发者、客户端、过期时间及 nonce。
pub async fn verify_identity(
    client: &reqwest::Client,
    tokens: &TokenResponse,
    client_id: &str,
    nonce: &str,
) -> Result<Identity, AuthError> {
    if !tokens
        .scope
        .split_whitespace()
        .any(|scope| scope == "chatgpt.tokens.use.direct")
        || !tokens.token_type.eq_ignore_ascii_case("bearer")
    {
        return Err(AuthError::InvalidIdentity);
    }
    let header = decode_header(&tokens.id_token).map_err(|_| AuthError::InvalidIdentity)?;
    if header.alg != Algorithm::RS256 {
        return Err(AuthError::InvalidIdentity);
    }
    let kid = header.kid.ok_or(AuthError::InvalidIdentity)?;
    let jwks: JwkSet = client
        .get("https://auth.openai.com/.well-known/jwks.json")
        .send()
        .await
        .map_err(|_| AuthError::InvalidIdentity)?
        .error_for_status()
        .map_err(|_| AuthError::InvalidIdentity)?
        .json()
        .await
        .map_err(|_| AuthError::InvalidIdentity)?;
    let key = DecodingKey::from_jwk(jwks.find(&kid).ok_or(AuthError::InvalidIdentity)?)
        .map_err(|_| AuthError::InvalidIdentity)?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&["https://auth.openai.com"]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    validation.leeway = 5;
    let claims = decode::<Claims>(&tokens.id_token, &key, &validation)
        .map_err(|_| AuthError::InvalidIdentity)?
        .claims;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| AuthError::InvalidIdentity)?
        .as_secs();
    if claims.nonce != nonce || claims.sub.is_empty() || claims.iat > now.saturating_add(5) {
        return Err(AuthError::InvalidIdentity);
    }
    Ok(Identity {
        subject: claims.sub,
        email: claims.email,
    })
}
