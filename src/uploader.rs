use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct Repository {
    id: String,
    name: String,
    user_id: String,
}

pub async fn login() -> Result<(), Box<dyn std::error::Error>> {
    let resp = reqwest::get("http://localhost:3000/repositories/")
        .await?
        .json::<Vec<Repository>>()
        .await?;
    println!("{:?}", resp);
    Ok(())
}
