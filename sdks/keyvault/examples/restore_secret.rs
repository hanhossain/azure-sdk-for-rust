use azure_auth_aad::ClientSecretCredential;
use azure_keyvault::KeyVaultClient;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client_id = env::var("CLIENT_ID").expect("Missing CLIENT_ID environment variable.");
    let client_secret =
        env::var("CLIENT_SECRET").expect("Missing CLIENT_SECRET environment variable.");
    let tenant_id = env::var("TENANT_ID").expect("Missing TENANT_ID environment variable.");
    let keyvault_name =
        env::var("KEYVAULT_NAME").expect("Missing KEYVAULT_NAME environment variable.");
    let backup_blob = env::var("BACKUP_BLOB").expect("Missing BACKUP_BLOB environment variable.");

    let creds = ClientSecretCredential::new(tenant_id, client_id, client_secret);
    let mut client = KeyVaultClient::new(&creds, &keyvault_name);

    client.restore_secret(&backup_blob).await?;

    Ok(())
}