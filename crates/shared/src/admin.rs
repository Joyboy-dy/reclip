//! Types for API keys and access management.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A permission granted to an API key.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
pub enum Scope {
    #[serde(rename = "probe")]
    Probe,
    #[serde(rename = "jobs:create")]
    #[ts(rename = "jobs:create")]
    JobsCreate,
    #[serde(rename = "jobs:read")]
    #[ts(rename = "jobs:read")]
    JobsRead,
    #[serde(rename = "jobs:cancel")]
    #[ts(rename = "jobs:cancel")]
    JobsCancel,
    #[serde(rename = "delegate")]
    Delegate,
}

impl Scope {
    /// Scopes given to a key created without an explicit list: everything
    /// except `delegate`.
    pub fn default_set() -> Vec<Scope> {
        vec![
            Scope::Probe,
            Scope::JobsCreate,
            Scope::JobsRead,
            Scope::JobsCancel,
        ]
    }
}

/// Per-key quota overrides. `None` means "use the default".
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq, TS)]
pub struct KeyLimits {
    #[serde(default)]
    pub probes_per_minute: Option<u32>,
    #[serde(default)]
    pub jobs_per_hour: Option<u32>,
    #[serde(default)]
    pub max_active: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct CreateKeyRequest {
    pub name: String,
    #[serde(default)]
    #[ts(optional)]
    pub scopes: Option<Vec<Scope>>,
    #[serde(default)]
    pub unlimited: bool,
    #[serde(default)]
    #[ts(optional)]
    pub expires_in_days: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub limits: Option<KeyLimits>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ApiKeyInfo {
    pub id: String,
    pub name: String,
    pub scopes: Vec<Scope>,
    pub unlimited: bool,
    /// Own limits; every field is null when the key has none.
    pub limits: KeyLimits,
    /// RFC 3339 timestamp.
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
    #[ts(type = "number")]
    pub request_count: u64,
}

/// Returned once at creation: the only time the secret is visible.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct CreatedKey {
    pub key: String,
    pub info: ApiKeyInfo,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ApiKeyList {
    pub keys: Vec<ApiKeyInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct AdminLogin {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct AdminMe {
    pub email: String,
    pub must_change_password: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct AdminPasswordChange {
    pub current: String,
    pub new: String,
    /// Also end the admin's other sessions.
    pub sign_out_others: bool,
}
