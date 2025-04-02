use actix_cors::Cors;
use actix_web::http;

mod rate_limiter;

pub fn configure_cors(allowed_origins: &[String]) -> Cors {
    let mut cors = Cors::default()
        .allowed_methods(vec!["GET", "POST"])
        .allowed_headers(vec![
            http::header::AUTHORIZATION,
            http::header::CONTENT_TYPE,
        ])
        .max_age(3600);

    // Add allowed origins
    if allowed_origins.is_empty() {
        // Default to localhost if no origins specified
        cors = cors.allowed_origin("http://localhost:3000");
    } else {
        for origin in allowed_origins {
            cors = cors.allowed_origin(origin);
        }
    }

    cors
}
