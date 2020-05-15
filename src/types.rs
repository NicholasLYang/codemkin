use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub repository_id: String,
    pub token_credentials: Option<TokenCredentials>,
    pub is_pushed: bool,
}

impl Config {
    pub fn new() -> Config {
        Config {
            repository_id: Uuid::new_v4().to_string(),
            token_credentials: None,
            is_pushed: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Repository {
    id: String,
    name: String,
    user_id: String,
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
