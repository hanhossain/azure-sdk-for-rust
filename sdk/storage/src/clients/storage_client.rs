use crate::authorization_policy::AuthorizationPolicy;
use crate::shared_access_signature::account_sas::{
    AccountSasPermissions, AccountSasResource, AccountSasResourceType, AccountSharedAccessSignature,
};
use crate::ConnectionString;
use azure_core::{
    auth::TokenCredential,
    error::{Error, ErrorKind, ResultExt},
    headers::*,
    Body, ClientOptions, Context, Method, Pipeline, Request, Response,
};
use azure_core::{date, Policy, TransportOptions};
use std::sync::Arc;
use time::OffsetDateTime;
use url::Url;

/// The well-known account used by Azurite and the legacy Azure Storage Emulator.
/// <https://docs.microsoft.com/azure/storage/common/storage-use-azurite#well-known-storage-account-and-key>
pub const EMULATOR_ACCOUNT: &str = "devstoreaccount1";

/// The well-known account key used by Azurite and the legacy Azure Storage Emulator.
/// <https://docs.microsoft.com/azure/storage/common/storage-use-azurite#well-known-storage-account-and-key>
pub const EMULATOR_ACCOUNT_KEY: &str =
    "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

const AZURE_VERSION: HeaderValue = HeaderValue::from_static("2019-12-12");

#[derive(Clone)]
pub enum StorageCredentials {
    Key(String, String),
    SASToken(Vec<(String, String)>),
    BearerToken(String),
    TokenCredential(Arc<dyn TokenCredential>),
    Anonymous,
}

impl StorageCredentials {
    /// Create an Access Key based credential
    ///
    /// When you create a storage account, Azure generates two 512-bit storage
    /// account access keys for that account. These keys can be used to
    /// authorize access to data in your storage account via Shared Key
    /// authorization.
    ///
    /// ref: <https://docs.microsoft.com/azure/storage/common/storage-account-keys-manage>
    pub fn access_key<A, K>(account: A, key: K) -> Self
    where
        A: Into<String>,
        K: Into<String>,
    {
        Self::Key(account.into(), key.into())
    }

    /// Create a Shared Access Signature (SAS) token based credential
    ///
    /// SAS token are HTTP query strings that provide delegated access to
    /// resources in a storage account with granular control over how the client
    /// can access data in the account.
    ///
    /// * ref: [Grant limited access to Azure Storage resources using shared access signatures (SAS)](https://docs.microsoft.com/azure/storage/common/storage-sas-overview)
    /// * ref: [Create SAS tokens for storage containers](https://docs.microsoft.com/azure/applied-ai-services/form-recognizer/create-sas-tokens)
    pub fn sas_token<S>(token: S) -> azure_core::Result<Self>
    where
        S: AsRef<str>,
    {
        let params = get_sas_token_parms(token.as_ref())?;
        Ok(Self::SASToken(params))
    }

    /// Create an Bearer Token based credential
    ///
    /// Azure Storage accepts OAuth 2.0 access tokens from the Azure AD tenant
    /// associated with the subscription that contains the storage account.
    ///
    /// While `StorageCredentials::TokenCredential` is the preferred way to
    /// manage access tokens, this method is provided for manual management of
    /// Oauth2 tokens.
    ///
    /// ref: <https://docs.microsoft.com/rest/api/storageservices/authorize-with-azure-active-directory>
    pub fn bearer_token<T>(token: T) -> Self
    where
        T: Into<String>,
    {
        Self::BearerToken(token.into())
    }

    /// Create a TokenCredential based credential
    ///
    /// Azure Storage accepts OAuth 2.0 access tokens from the Azure AD tenant
    /// associated with the subscription that contains the storage account.
    ///
    /// Token Credentials can be created and automatically updated using
    /// `azure_identity`.
    ///
    /// ```
    /// use azure_identity::DefaultAzureCredential;
    /// use azure_storage::prelude::*;
    /// use std::sync::Arc;
    /// let token_credential = Arc::new(DefaultAzureCredential::default());
    /// let storage_credentials = StorageCredentials::token_credential(token_credential);
    /// ```
    ///
    /// ref: <https://docs.microsoft.com/rest/api/storageservices/authorize-with-azure-active-directory>
    pub fn token_credential(credential: Arc<dyn TokenCredential>) -> Self {
        Self::TokenCredential(credential)
    }

    /// Create an anonymous credential
    ///
    /// Azure Storage supports optional anonymous public read access for
    /// containers and blobs. By default, anonymous access to data in a storage
    /// account data is not permitted. Unless anonymous access is explicitly
    /// enabled, all requests to a container and its blobs must be authorized.
    /// When a container's public access level setting is configured to permit
    /// anonymous access, clients can read data in that container without
    /// authorizing the request.
    ///
    /// ref: <https://docs.microsoft.com/azure/storage/blobs/anonymous-read-access-configure>
    pub fn anonymous() -> Self {
        Self::Anonymous
    }
}

impl std::fmt::Debug for StorageCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            StorageCredentials::Key(_, _) => f
                .debug_struct("StorageCredentials")
                .field("credential", &"Key")
                .finish(),
            StorageCredentials::SASToken(_) => f
                .debug_struct("StorageCredentials")
                .field("credential", &"SASToken")
                .finish(),
            StorageCredentials::BearerToken(_) => f
                .debug_struct("StorageCredentials")
                .field("credential", &"BearerToken")
                .finish(),
            StorageCredentials::TokenCredential(_) => f
                .debug_struct("StorageCredentials")
                .field("credential", &"TokenCredential")
                .finish(),
            StorageCredentials::Anonymous => f
                .debug_struct("StorageCredentials")
                .field("credential", &"Anonymous")
                .finish(),
        }
    }
}

impl From<Arc<dyn TokenCredential>> for StorageCredentials {
    fn from(cred: Arc<dyn TokenCredential>) -> Self {
        Self::TokenCredential(cred)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ServiceType {
    Blob,
    Queue,
    // File,
    Table,
    DataLake,
}

impl ServiceType {
    pub fn subdomain(&self) -> &str {
        match self {
            ServiceType::Blob => "blob",
            ServiceType::Queue => "queue",
            ServiceType::Table => "table",
            ServiceType::DataLake => "dfs",
        }
    }
}

#[derive(Clone, Debug)]
pub struct StorageClient {
    storage_credentials: StorageCredentials,
    blob_storage_url: Url,
    table_storage_url: Url,
    queue_storage_url: Url,
    queue_storage_secondary_url: Url,
    filesystem_url: Url,
    account: String,
    pipeline: Pipeline,
}

impl StorageClient {
    pub fn new_access_key<A, K>(account: A, key: K) -> Self
    where
        A: Into<String>,
        K: Into<String>,
    {
        let account = account.into();
        let storage_credentials = StorageCredentials::access_key(account.clone(), key);
        let pipeline =
            new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());

        Self {
            blob_storage_url: get_endpoint_uri(None, &account, "blob").unwrap(),
            table_storage_url: get_endpoint_uri(None, &account, "table").unwrap(),
            queue_storage_url: get_endpoint_uri(None, &account, "queue").unwrap(),
            queue_storage_secondary_url: get_endpoint_uri(
                None,
                &format!("{account}-secondary"),
                "queue",
            )
            .unwrap(),
            filesystem_url: get_endpoint_uri(None, &account, "dfs").unwrap(),
            storage_credentials,
            account,
            pipeline,
        }
    }

    /// Create a new client for customized emulator endpoints.
    pub fn new_emulator(
        blob_storage_url: &Url,
        table_storage_url: &Url,
        queue_storage_url: &Url,
        filesystem_url: &Url,
    ) -> Self {
        Self::new_emulator_with_account(
            blob_storage_url,
            table_storage_url,
            queue_storage_url,
            filesystem_url,
            EMULATOR_ACCOUNT,
            EMULATOR_ACCOUNT_KEY,
        )
    }

    /// Create a new client using the default HttpClient and the default emulator endpoints.
    pub fn new_emulator_default() -> Self {
        let blob_storage_url = Url::parse("http://127.0.0.1:10000").unwrap();
        let queue_storage_url = Url::parse("http://127.0.0.1:10001").unwrap();
        let table_storage_url = Url::parse("http://127.0.0.1:10002").unwrap();
        let filesystem_url = Url::parse("http://127.0.0.1:10004").unwrap();
        Self::new_emulator(
            &blob_storage_url,
            &table_storage_url,
            &queue_storage_url,
            &filesystem_url,
        )
    }

    pub fn new_emulator_with_account<A, K>(
        blob_storage_url: &Url,
        table_storage_url: &Url,
        queue_storage_url: &Url,
        filesystem_url: &Url,
        account: A,
        key: K,
    ) -> Self
    where
        A: Into<String>,
        K: Into<String>,
    {
        let account = account.into();
        let storage_credentials = StorageCredentials::access_key(account.clone(), key);
        let pipeline =
            new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());
        let blob_storage_url = Url::parse(&format!("{}{}", blob_storage_url, account)).unwrap();
        let table_storage_url = Url::parse(&format!("{}{}", table_storage_url, account)).unwrap();
        let queue_storage_url = Url::parse(&format!("{}{}", queue_storage_url, account)).unwrap();
        let filesystem_url = Url::parse(&format!("{}{}", filesystem_url, account)).unwrap();

        Self {
            blob_storage_url,
            table_storage_url,
            queue_storage_url: queue_storage_url.clone(),
            queue_storage_secondary_url: queue_storage_url,
            filesystem_url,
            storage_credentials,
            account,
            pipeline,
        }
    }

    pub fn new_sas_token<A, S>(account: A, sas_token: S) -> azure_core::Result<Self>
    where
        A: Into<String>,
        S: AsRef<str>,
    {
        let account = account.into();

        let storage_credentials = StorageCredentials::sas_token(sas_token)?;
        let pipeline =
            new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());

        Ok(Self {
            blob_storage_url: get_endpoint_uri(None, &account, "blob")?,
            table_storage_url: get_endpoint_uri(None, &account, "table")?,
            queue_storage_url: get_endpoint_uri(None, &account, "queue")?,
            queue_storage_secondary_url: get_endpoint_uri(
                None,
                &format!("{account}-secondary"),
                "queue",
            )?,
            filesystem_url: get_endpoint_uri(None, &account, "dfs")?,
            storage_credentials,
            account,
            pipeline,
        })
    }

    pub fn new_bearer_token<A, BT>(account: A, bearer_token: BT) -> Self
    where
        A: Into<String>,
        BT: Into<String>,
    {
        let account = account.into();
        let storage_credentials = StorageCredentials::bearer_token(bearer_token);
        let pipeline =
            new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());

        Self {
            blob_storage_url: get_endpoint_uri(None, &account, "blob").unwrap(),
            table_storage_url: get_endpoint_uri(None, &account, "table").unwrap(),
            queue_storage_url: get_endpoint_uri(None, &account, "queue").unwrap(),
            queue_storage_secondary_url: get_endpoint_uri(
                None,
                &format!("{}-secondary", account),
                "queue",
            )
            .unwrap(),
            filesystem_url: get_endpoint_uri(None, &account, "dfs").unwrap(),
            storage_credentials,
            account,
            pipeline,
        }
    }

    pub fn new_token_credential<A>(account: A, token_credential: Arc<dyn TokenCredential>) -> Self
    where
        A: Into<String>,
    {
        let account = account.into();
        let storage_credentials = StorageCredentials::token_credential(token_credential);
        let pipeline =
            new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());

        Self {
            blob_storage_url: get_endpoint_uri(None, &account, "blob").unwrap(),
            table_storage_url: get_endpoint_uri(None, &account, "table").unwrap(),
            queue_storage_url: get_endpoint_uri(None, &account, "queue").unwrap(),
            queue_storage_secondary_url: get_endpoint_uri(
                None,
                &format!("{}-secondary", account),
                "queue",
            )
            .unwrap(),
            filesystem_url: get_endpoint_uri(None, &account, "dfs").unwrap(),
            storage_credentials,
            account,
            pipeline,
        }
    }

    pub fn new_connection_string(connection_string: &str) -> azure_core::Result<Self> {
        match ConnectionString::new(connection_string)? {
            ConnectionString {
                account_name: Some(account),
                account_key: Some(_),
                sas: Some(sas_token),
                blob_endpoint,
                table_endpoint,
                queue_endpoint,
                file_endpoint,
                ..
            } => {
                log::warn!("Both account key and SAS defined in connection string. Using only the provided SAS.");

                let storage_credentials =  StorageCredentials::sas_token(sas_token)?;
                let pipeline = new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());

                Ok(Self {
                    storage_credentials,
                    blob_storage_url: get_endpoint_uri(blob_endpoint, account, "blob")?,
                    table_storage_url: get_endpoint_uri(table_endpoint, account, "table")?,
                    queue_storage_url: get_endpoint_uri(queue_endpoint, account, "queue")?,
                    queue_storage_secondary_url: get_endpoint_uri(queue_endpoint, &format!("{}-secondary", account), "queue")?,
                    filesystem_url: get_endpoint_uri(file_endpoint, account, "dfs")?,
                    account: account.to_string(),
                    pipeline
                })
            }
            ConnectionString {
                account_name: Some(account),
                sas: Some(sas_token),
                blob_endpoint,
                table_endpoint,
                queue_endpoint,
                file_endpoint,
                ..
            } => {
                let storage_credentials = StorageCredentials::sas_token(sas_token)?;
                let pipeline =
                new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());
                Ok(Self {
                    storage_credentials,
                    blob_storage_url: get_endpoint_uri(blob_endpoint, account, "blob")?,
                    table_storage_url: get_endpoint_uri(table_endpoint, account, "table")?,
                    queue_storage_url: get_endpoint_uri(queue_endpoint, account, "queue")?,
                    queue_storage_secondary_url: get_endpoint_uri(queue_endpoint, &format!("{}-secondary", account), "queue")?,
                    filesystem_url: get_endpoint_uri(file_endpoint, account, "dfs")?,
                    account: account.to_string(),
                    pipeline
            })},
            ConnectionString {
                account_name: Some(account),
                account_key: Some(key),
                blob_endpoint,
                table_endpoint,
                queue_endpoint,
                file_endpoint,
                ..
            } => {

                let storage_credentials = StorageCredentials::access_key(account, key);
                let pipeline = new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());
                Ok(Self {
                storage_credentials,
                blob_storage_url: get_endpoint_uri(blob_endpoint, account, "blob")?,
                table_storage_url: get_endpoint_uri(table_endpoint, account, "table")?,
                queue_storage_url: get_endpoint_uri(queue_endpoint, account, "queue")?,
                queue_storage_secondary_url: get_endpoint_uri(queue_endpoint, &format!("{}-secondary", account), "queue")?,
                filesystem_url: get_endpoint_uri(file_endpoint, account, "dfs")?,
                account: account.to_string(),
                pipeline
            })
        },
           _ => {
                Err(Error::message(ErrorKind::Other,
                    "Could not create a storage client from the provided connection string. Please validate that you have specified the account name and means of authentication (key, SAS, etc.)."
                ))
            }
        }
    }

    /// Create a new anonymous storage client
    ///
    /// ref: <https://docs.microsoft.com/en-us/azure/storage/blobs/anonymous-read-access-configure?tabs=portal>
    pub fn new_anonymous<A>(account: A) -> Self
    where
        A: Into<String>,
    {
        let account = account.into();
        let storage_credentials = StorageCredentials::anonymous();
        let pipeline =
            new_pipeline_from_options(ClientOptions::default(), storage_credentials.clone());

        Self {
            blob_storage_url: get_endpoint_uri(None, &account, "blob").unwrap(),
            table_storage_url: get_endpoint_uri(None, &account, "table").unwrap(),
            queue_storage_url: get_endpoint_uri(None, &account, "queue").unwrap(),
            queue_storage_secondary_url: get_endpoint_uri(
                None,
                &format!("{}-secondary", account),
                "queue",
            )
            .unwrap(),
            filesystem_url: get_endpoint_uri(None, &account, "dfs").unwrap(),
            storage_credentials,
            account,
            pipeline,
        }
    }

    /// Create a new instance of `StorageClient` using a mock backend. The
    /// transaction name is used to look up which files to read to validate the
    /// request and mock the response.
    pub fn new_mock(
        account: impl Into<String>,
        storage_credentials: StorageCredentials,
        transport_policy: Arc<dyn Policy>,
    ) -> Self {
        let account = account.into();
        let options = ClientOptions::new(TransportOptions::new_custom_policy(transport_policy));
        let pipeline = new_pipeline_from_options(options, storage_credentials.clone());
        Self {
            blob_storage_url: get_endpoint_uri(None, &account, "blob").unwrap(),
            table_storage_url: get_endpoint_uri(None, &account, "table").unwrap(),
            queue_storage_url: get_endpoint_uri(None, &account, "queue").unwrap(),
            queue_storage_secondary_url: get_endpoint_uri(
                None,
                &format!("{}-secondary", account),
                "queue",
            )
            .unwrap(),
            filesystem_url: get_endpoint_uri(None, &account, "dfs").unwrap(),
            storage_credentials,
            account,
            pipeline,
        }
    }

    pub fn blob_storage_url(&self) -> &Url {
        &self.blob_storage_url
    }

    pub fn table_storage_url(&self) -> &Url {
        &self.table_storage_url
    }

    pub fn queue_storage_url(&self) -> &Url {
        &self.queue_storage_url
    }

    pub fn queue_storage_secondary_url(&self) -> &Url {
        &self.queue_storage_secondary_url
    }

    pub fn filesystem_url(&self) -> &Url {
        &self.filesystem_url
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn storage_credentials(&self) -> &StorageCredentials {
        &self.storage_credentials
    }

    pub fn finalize_request(
        &self,
        url: Url,
        method: Method,
        headers: Headers,
        request_body: Option<Body>,
    ) -> azure_core::Result<Request> {
        finalize_request(url, method, headers, request_body)
    }

    pub async fn send(
        &self,
        context: &mut Context,
        request: &mut Request,
        service_type: ServiceType,
    ) -> azure_core::Result<Response> {
        self.pipeline
            .send(context.insert(service_type), request)
            .await
    }

    pub fn shared_access_signature(
        &self,
        resource: AccountSasResource,
        resource_type: AccountSasResourceType,
        expiry: OffsetDateTime,
        permissions: AccountSasPermissions,
    ) -> azure_core::Result<AccountSharedAccessSignature> {
        shared_access_signature(
            &self.storage_credentials,
            resource,
            resource_type,
            expiry,
            permissions,
        )
    }

    pub fn blob_url_with_segments<'a, I>(&'a self, segments: I) -> azure_core::Result<url::Url>
    where
        I: IntoIterator<Item = &'a str>,
    {
        Self::url_with_segments(self.blob_storage_url().to_owned(), segments)
    }

    pub fn queue_url_with_segments<'a, I>(&'a self, segments: I) -> azure_core::Result<url::Url>
    where
        I: IntoIterator<Item = &'a str>,
    {
        Self::url_with_segments(self.queue_storage_url().to_owned(), segments)
    }

    pub fn url_with_segments<'a, I>(
        mut url: url::Url,
        new_segments: I,
    ) -> azure_core::Result<url::Url>
    where
        I: IntoIterator<Item = &'a str>,
    {
        let original_url = url.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                let message = format!("failed to parse url path segments from '{original_url}'");
                Error::message(ErrorKind::DataConversion, message)
            })?;
            segments.extend(new_segments);
        }
        Ok(url)
    }
}

pub fn shared_access_signature(
    storage_credentials: &StorageCredentials,
    resource: AccountSasResource,
    resource_type: AccountSasResourceType,
    expiry: OffsetDateTime,
    permissions: AccountSasPermissions,
) -> Result<AccountSharedAccessSignature, Error> {
    match storage_credentials {
            StorageCredentials::Key(account, key) => {
                Ok(AccountSharedAccessSignature::new(account.clone(), key.clone(), resource, resource_type, expiry, permissions))
            }
            _ => Err(Error::message(ErrorKind::Credential, "failed shared access signature generation. SAS can be generated only from key and account clients")),
        }
}

pub fn finalize_request(
    url: Url,
    method: Method,
    headers: Headers,
    request_body: Option<Body>,
) -> Result<Request, Error> {
    let dt = OffsetDateTime::now_utc();
    let time = date::to_rfc1123(&dt);
    let mut request = Request::new(url, method);
    for (k, v) in headers {
        request.insert_header(k, v);
    }
    // let's add content length to avoid "chunking" errors.
    match request_body {
        Some(ref b) => request.insert_header(CONTENT_LENGTH, b.len().to_string()),
        None => request.insert_header(CONTENT_LENGTH, "0"),
    };
    request.insert_header(MS_DATE, time);
    request.insert_header(VERSION, AZURE_VERSION);
    if let Some(request_body) = request_body {
        request.set_body(request_body);
    } else {
        request.set_body(azure_core::EMPTY_BODY);
    };
    Ok(request)
}

fn get_sas_token_parms(sas_token: &str) -> azure_core::Result<Vec<(String, String)>> {
    // Any base url will do: we just need to parse the SAS token
    // to get its query pairs.
    let base_url = Url::parse("https://blob.core.windows.net").unwrap();

    let url = Url::options().base_url(Some(&base_url));

    // this code handles the leading ?
    // we support both with or without
    let url = if sas_token.starts_with('?') {
        url.parse(sas_token)
    } else {
        url.parse(&format!("?{}", sas_token))
    }
    .with_context(ErrorKind::DataConversion, || {
        format!("failed to parse SAS token: {sas_token}")
    })?;

    Ok(url
        .query_pairs()
        .map(|p| (String::from(p.0), String::from(p.1)))
        .collect())
}

fn get_endpoint_uri(
    url: Option<&str>,
    account: &str,
    endpoint_type: &str,
) -> azure_core::Result<url::Url> {
    Ok(match url {
        Some(value) => url::Url::parse(value)?,
        None => url::Url::parse(&format!(
            "https://{}.{}.core.windows.net",
            account, endpoint_type
        ))
        .with_context(ErrorKind::DataConversion, || {
            format!("failed to parse url: https://{account}.{endpoint_type}.core.windows.net")
        })?,
    })
}

/// Create a Pipeline from ClientOptions
pub fn new_pipeline_from_options(
    options: ClientOptions,
    credentials: StorageCredentials,
) -> Pipeline {
    let auth_policy: Arc<dyn azure_core::Policy> = Arc::new(AuthorizationPolicy::new(credentials));

    // The `AuthorizationPolicy` must be the **last** retry policy.
    // Policies can change the url and/or the headers, and the `AuthorizationPolicy`
    // must be able to inspect them or the resulting token will be invalid.
    let per_retry_policies = vec![
        Arc::new(options.timeout.clone()) as Arc<dyn azure_core::Policy>,
        auth_policy,
    ];

    Pipeline::new(
        option_env!("CARGO_PKG_NAME"),
        option_env!("CARGO_PKG_VERSION"),
        options,
        Vec::new(),
        per_retry_policies,
    )
}
