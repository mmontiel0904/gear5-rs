use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Shape of every error response: `ApiError::into_response` always emits this JSON body.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ErrorBody {
    /// Human-readable failure description. Stable enough to switch on for known cases
    /// (`"invalid or missing api key"`, `"missing required scope"`, `"not found"`,
    /// `"rate limit exceeded"`).
    pub error: String,
}
