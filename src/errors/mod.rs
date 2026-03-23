use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, warn};

/// Describes what kind of API error we hit
#[derive(Debug, Clone)]
pub enum ApiError {
    RateLimit { retry_after_secs: u64, provider: String },
    DailyCapExceeded { provider: String },
    InvalidKey { provider: String },
    ContextTooLarge { provider: String },
    ServiceUnavailable { provider: String },
    Unknown { status: u16, body: String },
}

impl ApiError {
    /// Parse a raw HTTP error response into a structured ApiError
    pub fn from_response(status: u16, body: &str, provider: &str) -> Self {
        if status == 429 {
            // Try to extract retry-after from the body
            let retry = extract_retry_seconds(body).unwrap_or(60);

            if body.contains("per day") || body.contains("TPD") || body.contains("daily") {
                return ApiError::DailyCapExceeded { provider: provider.to_string() };
            }

            return ApiError::RateLimit {
                retry_after_secs: retry,
                provider: provider.to_string(),
            };
        }

        if status == 401 || status == 403 {
            return ApiError::InvalidKey { provider: provider.to_string() };
        }

        if status == 400 && (body.contains("context") || body.contains("too long") || body.contains("tokens")) {
            return ApiError::ContextTooLarge { provider: provider.to_string() };
        }

        if status >= 500 {
            return ApiError::ServiceUnavailable { provider: provider.to_string() };
        }

        ApiError::Unknown { status, body: body.chars().take(200).collect() }
    }

    /// User-friendly message — no jargon, actionable
    pub fn user_message(&self) -> String {
        match self {
            ApiError::RateLimit { retry_after_secs, provider } => format!(
                "⏳ {} rate limit hit — backing off for {}s. This is normal at high poll rates.",
                provider, retry_after_secs
            ),
            ApiError::DailyCapExceeded { provider } => format!(
                "🚫 {} daily token cap reached. openmaven will pause AI calls until midnight UTC.\n\
                → To avoid this: increase poll interval, reduce subreddits, or upgrade your API tier.",
                provider
            ),
            ApiError::InvalidKey { provider } => format!(
                "🔑 {} API key is invalid or expired. Run `openmaven setup` to update it.",
                provider
            ),
            ApiError::ContextTooLarge { provider } => format!(
                "📄 {} context window exceeded. Reducing batch size automatically.",
                provider
            ),
            ApiError::ServiceUnavailable { provider } => format!(
                "⚡ {} is temporarily unavailable. Will retry automatically.",
                provider
            ),
            ApiError::Unknown { status, body } => format!(
                "❓ Unexpected API error (HTTP {}): {}",
                status, body
            ),
        }
    }

    /// How long to wait before retrying (seconds)
    pub fn backoff_secs(&self) -> u64 {
        match self {
            ApiError::RateLimit { retry_after_secs, .. } => *retry_after_secs,
            ApiError::DailyCapExceeded { .. } => 3600, // wait an hour
            ApiError::InvalidKey { .. } => 0,          // no point retrying
            ApiError::ContextTooLarge { .. } => 0,     // needs config change
            ApiError::ServiceUnavailable { .. } => 30,
            ApiError::Unknown { .. } => 15,
        }
    }

    pub fn is_retryable(&self) -> bool {
        !matches!(self, ApiError::InvalidKey { .. })
    }
}

/// Shared error state per provider — tracks consecutive failures and backoff
#[derive(Debug, Default)]
pub struct ProviderHealth {
    pub consecutive_failures: u32,
    pub backoff_until: Option<std::time::Instant>,
    pub daily_cap_hit: bool,
    pub last_error: Option<String>,
}

impl ProviderHealth {
    pub fn is_available(&self) -> bool {
        if self.daily_cap_hit {
            return false;
        }
        if let Some(until) = self.backoff_until {
            return std::time::Instant::now() >= until;
        }
        true
    }

    pub fn record_error(&mut self, err: &ApiError) {
        self.consecutive_failures += 1;
        self.last_error = Some(err.user_message());

        let backoff = err.backoff_secs();
        if backoff > 0 {
            self.backoff_until = Some(
                std::time::Instant::now() + Duration::from_secs(backoff)
            );
        }

        if matches!(err, ApiError::DailyCapExceeded { .. }) {
            self.daily_cap_hit = true;
        }
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.backoff_until = None;
        self.last_error = None;
    }
}

pub type SharedProviderHealth = Arc<Mutex<ProviderHealth>>;

pub fn new_provider_health() -> SharedProviderHealth {
    Arc::new(Mutex::new(ProviderHealth::default()))
}

/// Check a raw HTTP response and return an ApiError if it indicates failure
pub fn check_response(status: u16, body: &str, provider: &str) -> Option<ApiError> {
    if status == 200 { return None; }
    Some(ApiError::from_response(status, body, provider))
}

/// Log an API error with context, return the user-friendly message
pub fn log_api_error(err: &ApiError, context: &str) -> String {
    let msg = err.user_message();
    match err {
        ApiError::InvalidKey { .. } => error!("[{}] {}", context, msg),
        ApiError::DailyCapExceeded { .. } => error!("[{}] {}", context, msg),
        ApiError::RateLimit { .. } => warn!("[{}] {}", context, msg),
        _ => warn!("[{}] {}", context, msg),
    }
    msg
}

fn extract_retry_seconds(body: &str) -> Option<u64> {
    // Groq format: "Please try again in 2.15s"
    // Also handles "in 1m30s", "in 7m33.6s"
    if let Some(idx) = body.find("try again in ") {
        let rest = &body[idx + 13..];
        let end = rest.find(['.', 's', '"', '}']).unwrap_or(rest.len());
        let token = &rest[..end];

        // Handle "Xm" or "XmYs" format
        if let Some(m_idx) = token.find('m') {
            let mins: u64 = token[..m_idx].trim().parse().unwrap_or(0);
            let secs_part = &token[m_idx + 1..];
            let secs: u64 = secs_part.trim_end_matches('s').trim().parse().unwrap_or(0);
            return Some(mins * 60 + secs + 2);
        }

        if let Ok(secs) = token.trim().parse::<f64>() {
            return Some(secs.ceil() as u64 + 1);
        }
    }
    None
}
