use crate::types::{
    Change, ChangeRequest, Document, DocumentRequest, LoginCredentials, Repository,
    RepositoryRequest, TokenCredentials,
};
use anyhow::Result;
use futures::future::try_join_all;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use rusqlite::{params, Connection, NO_PARAMS};
use std::convert::{TryFrom, TryInto};
use std::io::{self, stdout, Write};

fn read_line(s: &mut String, prompt: &str) -> Result<()> {
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
    type Error = anyhow::Error;
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

pub async fn login() -> Result<TokenCredentials> {
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

struct DocumentRow(i64, String);

async fn push_document(
    doc: DocumentRow,
    repo_id: &str,
    conn: &Connection,
    credentials: &TokenCredentials,
) -> Result<()> {
    let client = reqwest::Client::new();
    let create_url = format!("http://localhost:3000/repositories/{}/documents", repo_id);
    let payload = DocumentRequest { path: &doc.1 };
    let resp = client
        .post(&create_url)
        .json(&payload)
        .headers(make_auth_headers(credentials))
        .send()
        .await?;
    let document_response = resp.json::<Document>().await?;
    let url = format!(
        "http://localhost:3000/repositories/{}/documents/{}/last-change",
        repo_id, document_response.id
    );
    let resp = client
        .get(&url)
        .headers(make_auth_headers(credentials))
        .send()
        .await?;
    let change_url = format!(
        "http://localhost:3000/documents/{}/changes",
        document_response.id
    );
    let mut change_requests = Vec::new();
    if let Some(last_change) = resp.json::<Option<Change>>().await? {
        let mut query = conn.prepare(
            "SELECT change_elements FROM changes WHERE created_at > ?1 AND document_id = ?2",
        )?;
        let changes =
            query.query_map(
                params![last_change.created_at, doc.0],
                |row| Ok(row.get(0)?),
            )?;
        for change in changes {
            let change = change?;
            let payload = ChangeRequest {
                elements: change,
                document_id: &document_response.id,
            };

            change_requests.push(
                client
                    .post(&change_url)
                    .json(&payload)
                    .headers(make_auth_headers(credentials))
                    .send(),
            );
        }
    } else {
        let mut query =
            conn.prepare("SELECT change_elements FROM changes WHERE document_id = ?1")?;
        let changes = query.query_map(params![doc.0], |row| Ok(row.get(0)?))?;
        for change in changes {
            let change = change?;
            let payload = ChangeRequest {
                elements: change,
                document_id: &document_response.id,
            };
            change_requests.push(
                client
                    .post(&change_url)
                    .json(&payload)
                    .headers(make_auth_headers(credentials))
                    .send(),
            );
        }
    }
    if change_requests.len() > 0 {
        println!(
            "Uploading {} new changes for {}",
            change_requests.len(),
            &doc.1
        );
    }
    try_join_all(change_requests).await?;
    Ok(())
}

pub async fn push_repo(
    conn: &Connection,
    repo_id: &str,
    credentials: &TokenCredentials,
) -> Result<()> {
    let mut query = conn.prepare("SELECT id, path FROM documents")?;
    let documents = query.query_map(NO_PARAMS, |row| Ok(DocumentRow(row.get(0)?, row.get(1)?)))?;
    let mut document_requests = Vec::new();
    for doc in documents {
        let doc = doc?;
        document_requests.push(push_document(doc, repo_id, conn, credentials));
    }
    try_join_all(document_requests).await?;
    Ok(())
}

pub fn make_auth_headers(credentials: &TokenCredentials) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let token = HeaderName::from_static("access-token");
    let client = HeaderName::from_static("client");
    let expiry = HeaderName::from_static("expiry");
    let token_type = HeaderName::from_static("token-type");
    let uid = HeaderName::from_static("uid");
    headers.insert(token, HeaderValue::from_str(&credentials.token).unwrap());
    headers.insert(client, HeaderValue::from_str(&credentials.client).unwrap());
    headers.insert(expiry, HeaderValue::from_str(&credentials.expiry).unwrap());
    headers.insert(
        token_type,
        HeaderValue::from_str(&credentials.token_type).unwrap(),
    );
    headers.insert(uid, HeaderValue::from_str(&credentials.uid).unwrap());
    headers
}

pub async fn init_repo(credentials: &TokenCredentials) -> Result<(TokenCredentials, Repository)> {
    let client = reqwest::Client::new();
    let mut name = String::new();
    read_line(&mut name, "Repo name: ")?;
    let data = RepositoryRequest { name };
    let headers = make_auth_headers(credentials);
    let resp = client
        .post("http://localhost:3000/repositories")
        .headers(headers)
        .json(&data)
        .send()
        .await?;
    let creds: TokenCredentials = resp.headers().try_into()?;
    let repository = resp.json::<Repository>().await?;
    Ok((creds, repository))
}
