use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenCredentials {
    pub token: String,
    pub client: String,
    pub expiry: String,
    pub token_type: String,
    pub uid: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub id: String,
    pub token_credentials: Option<TokenCredentials>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct LoginCredentials {
    pub email: String,
    pub password: String,
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
pub struct ChangeRequest<'a> {
    pub elements: String,
    pub document_id: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Change {
    pub created_at: String,
}
