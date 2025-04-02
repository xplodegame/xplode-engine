use actix_web::{
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
    error, Error,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant},
};

// Rate limit configuration
pub struct RateLimiter {
    // Requests per minute allowed
    requests_per_minute: usize,
    // IP-based tracking
    pub ip_tracking: Arc<Mutex<HashMap<String, (usize, Instant)>>>,
}

impl RateLimiter {
    pub fn new(requests_per_minute: usize) -> Self {
        RateLimiter {
            requests_per_minute,
            ip_tracking: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for RateLimiter
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = RateLimiterMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(RateLimiterMiddleware {
            service,
            requests_per_minute: self.requests_per_minute,
            ip_tracking: self.ip_tracking.clone(),
        })
    }
}

pub struct RateLimiterMiddleware<S> {
    service: S,
    requests_per_minute: usize,
    ip_tracking: Arc<Mutex<HashMap<String, (usize, Instant)>>>,
}

impl<S, B> Service<ServiceRequest> for RateLimiterMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
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
        // Skip rate limiting for health checks
        if req.path() == "/health" {
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res)
            });
        }

        // Get client IP
        let ip = match req.connection_info().realip_remote_addr() {
            Some(ip) => ip.to_string(),
            None => "unknown".to_string(),
        };

        // Check rate limits
        let mut ip_map = self.ip_tracking.lock().unwrap();
        let now = Instant::now();

        let current = ip_map.get(&ip).copied();
        match current {
            Some((count, time)) => {
                // Reset counter if a minute has passed
                if now.duration_since(time) > Duration::from_secs(60) {
                    ip_map.insert(ip.clone(), (1, now));
                } else if count >= self.requests_per_minute {
                    // Rate limit exceeded
                    return Box::pin(async move {
                        Err(error::ErrorTooManyRequests(
                            "Rate limit exceeded. Try again later.",
                        ))
                    });
                } else {
                    // Increment counter
                    ip_map.insert(ip.clone(), (count + 1, time));
                }
            }
            None => {
                // First request from this IP
                ip_map.insert(ip.clone(), (1, now));
            }
        }

        // Process the request
        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res)
        })
    }
}
