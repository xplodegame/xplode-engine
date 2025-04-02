use super::Claims;
use actix_web::{
    dev::{self, Service, ServiceRequest, ServiceResponse, Transform},
    error, Error, HttpMessage,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use jsonwebtoken::{decode, DecodingKey, Validation};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub struct AuthenticationMiddleware {
    jwt_secret: String,
    exclude_routes: Vec<String>,
}

impl AuthenticationMiddleware {
    pub fn new(jwt_secret: String) -> Self {
        Self {
            jwt_secret,
            exclude_routes: vec!["/health".to_string(), "/login".to_string()],
        }
    }

    pub fn exclude(mut self, path: &str) -> Self {
        self.exclude_routes.push(path.to_string());
        self
    }
}

impl<S, B> Transform<S, ServiceRequest> for AuthenticationMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = AuthenticationMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AuthenticationMiddlewareService {
            service,
            jwt_secret: self.jwt_secret.clone(),
            exclude_routes: self.exclude_routes.clone(),
        })
    }
}

pub struct AuthenticationMiddlewareService<S> {
    service: S,
    jwt_secret: String,
    exclude_routes: Vec<String>,
}

impl<S, B> Service<ServiceRequest> for AuthenticationMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(cx)
    }

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let path = req.path().to_string();

        // Skip authentication for excluded routes
        if self.exclude_routes.contains(&path)
            || path.starts_with("/user-details") && req.method() == "POST"
        {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res)
            });
        }

        // Extract the token from Authorization header
        let auth_header = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|auth_str| {
                if auth_str.starts_with("Bearer ") {
                    Some(auth_str[7..].to_string())
                } else {
                    None
                }
            });

        // If no token, return unauthorized
        let token = match auth_header {
            Some(token) => token,
            None => {
                return Box::pin(async move {
                    Err(error::ErrorUnauthorized("Missing authorization token"))
                });
            }
        };

        // Validate the token
        let jwt_secret = self.jwt_secret.clone();
        let token_claims = match decode::<Claims>(
            &token,
            &DecodingKey::from_secret(jwt_secret.as_bytes()),
            &Validation::default(),
        ) {
            Ok(data) => data.claims,
            Err(e) => {
                return Box::pin(async move {
                    Err(error::ErrorUnauthorized(format!("Invalid token: {}", e)))
                });
            }
        };

        // Add claims to request extensions
        req.extensions_mut().insert(token_claims);

        // Call the next middleware
        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res)
        })
    }
}
