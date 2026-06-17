use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use std::{net::IpAddr, num::NonZeroU32, sync::Arc};
use crate::state::AppState;

pub type SharedRateLimiter = Arc<DefaultKeyedRateLimiter<IpAddr>>;

pub fn new_limiter(per_second: u32) -> SharedRateLimiter {
    let quota = Quota::per_second(
        NonZeroU32::new(per_second).expect("rate_limit_per_second must be >= 1"),
    );
    Arc::new(RateLimiter::keyed(quota))
}

pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(limiter) = &state.limiter {
        if limiter.check_key(&addr.ip()).is_err() {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }
    Ok(next.run(request).await)
}
