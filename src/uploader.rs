use crate::types::{LoginCredentials, TokenCredentials};
use reqwest::header::HeaderMap;
use std::convert::{TryFrom, TryInto};
use std::io::{self, stdout, Write};

fn read_line(s: &mut String, prompt: &str) -> Result<(), io::Error> {
    print!("{}", prompt);
    stdout().flush()?;
    io::stdin().read_line(s)?;
    if let Some('\n') = s.chars().next_back() {
        s.pop();
    }
    if let Some('\r') = s.chars().next_back() {
        s.pop();
    }
    Ok(())
}

impl TryFrom<&HeaderMap> for TokenCredentials {
    type Error = Box<dyn std::error::Error>;
    fn try_from(headers: &HeaderMap) -> Result<Self, Self::Error> {
        let token = headers
            .get("access-token")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let client = headers.get("client").unwrap().to_str().unwrap().to_string();
        let expiry = headers.get("expiry").unwrap().to_str().unwrap().to_string();
        let token_type = headers
            .get("token-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let uid = headers.get("uid").unwrap().to_str().unwrap().to_string();
        Ok(TokenCredentials {
            token,
            client,
            expiry,
            token_type,
            uid,
        })
    }
}

pub async fn login() -> Result<TokenCredentials, Box<dyn std::error::Error>> {
    let mut email = String::new();
    let mut password = String::new();
    read_line(&mut email, "Email: ")?;
    read_line(&mut password, "Password: ")?;

    let credentials = LoginCredentials { email, password };
    let client = reqwest::Client::new();
    let resp = client
        .post("http://localhost:3000/auth/sign_in")
        .json(&credentials)
        .send()
        .await?;
    let headers = resp.headers();
    let credentials: TokenCredentials = headers.try_into()?;
    Ok(credentials)
}
