use actix_web::{
    dev::Payload, error, web, Error, FromRequest, HttpMessage, HttpRequest, HttpResponse,
};
use chrono::{Duration, Utc};
use futures::future::{err, ok, Ready};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

mod middleware;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // Subject (user ID)
    pub exp: usize,  // Expiration timestamp
    pub iat: usize,  // Issued at timestamp
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LoginRequest {
    pub privy_id: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub token: String,
    pub expires_in: u64,
}

pub struct AuthenticatedUser {
    pub user_id: String,
}

impl FromRequest for AuthenticatedUser {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _: &mut Payload) -> Self::Future {
        // Extract the claims from the request extensions (set by auth middleware)
        match req.extensions().get::<Claims>() {
            Some(claims) => ok(AuthenticatedUser {
                user_id: claims.sub.clone(),
            }),
            None => err(error::ErrorUnauthorized("User not authenticated")),
        }
    }
}

pub fn create_token(user_id: &str, secret: &str, expiration_seconds: u64) -> Result<String, Error> {
    let now = Utc::now();
    let exp = (now + Duration::seconds(expiration_seconds as i64)).timestamp() as usize;
    let iat = now.timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        iat,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| error::ErrorInternalServerError(format!("Token creation error: {}", e)))
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims, Error> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
            error::ErrorUnauthorized("Token expired")
        }
        _ => error::ErrorUnauthorized("Invalid token"),
    })
}
