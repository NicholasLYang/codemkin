use chrono::Utc;
use difference::Difference;
use serde::{Deserialize, Serialize};

// TODO: Use this to activate/deactivate the watcher
// for specific repos
pub enum RepoStatus {
    Inactive = 0,
    Starting = 1,
    Active = 2,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenCredentials {
    pub token: String,
    pub client: String,
    pub expiry: String,
    pub token_type: String,
    pub uid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InternalConfig {
    pub id: String,
    pub token_credentials: Option<TokenCredentials>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct User {
    uid: String,
    id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    data: User,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepositoryRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentRequest<'a> {
    pub path: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangeRequest {
    pub elements: String,
    pub document_id: String,
    pub created_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BulkChangeRequest {
    pub changes: Vec<ChangeRequest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Change {
    pub created_at: chrono::DateTime<Utc>,
    pub change_elements: Vec<ChangeElement>,
}

// Sigh, to get around the orphan rule, we define
// our own difference type
#[derive(Debug, Serialize, Deserialize)]
pub struct ChangeElement {
    #[serde(rename = "type")]
    pub type_: ChangeType,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeType {
    Same,
    Add,
    Remove,
}

impl From<Difference> for ChangeElement {
    fn from(diff: Difference) -> Self {
        match diff {
            Difference::Same(s) => ChangeElement {
                type_: ChangeType::Same,
                content: s,
            },
            Difference::Rem(s) => ChangeElement {
                type_: ChangeType::Remove,
                content: s,
            },
            Difference::Add(s) => ChangeElement {
                type_: ChangeType::Add,
                content: s,
            },
        }
    }
}
