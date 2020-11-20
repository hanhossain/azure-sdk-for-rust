use crate::from_headers::*;
use crate::permission::CosmosPermission;
use crate::CosmosError;
use crate::Permission;
use azure_core::headers::{continuation_token_from_headers_optional, session_token_from_headers};
use http::response::Response;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub struct ListPermissionsResponse<'a> {
    pub permissions: Vec<Permission<'a, Cow<'a, str>>>,
    pub charge: f64,
    pub activity_id: uuid::Uuid,
    pub session_token: String,
    pub content_path: String,
    pub alt_content_path: String,
    pub continuation_token: Option<String>,
}

impl<'a> std::convert::TryFrom<Response<Vec<u8>>> for ListPermissionsResponse<'a> {
    type Error = CosmosError;

    fn try_from(response: Response<Vec<u8>>) -> Result<Self, Self::Error> {
        let headers = response.headers();
        let body = response.body();

        debug!("headers == {:#?}", headers);
        debug!("body == {:#?}", std::str::from_utf8(body)?);

        #[derive(Debug, Deserialize)]
        struct Response<'b> {
            _rid: String,
            #[serde(rename = "Permissions")]
            permissions: Vec<CosmosPermission<'b>>,
            _count: u32,
        }

        // first get the Cosmos REST API permission
        let response: Response<'_> = serde_json::from_slice(body)?;
        debug!("response == {:#?}", response);

        // now convert every Cosmos REST API permission
        // into the SDK struct
        let permissions = response
            .permissions
            .into_iter()
            .map(Permission::try_from)
            .collect::<Result<Vec<Permission<'_, Cow<'_, str>>>, CosmosError>>()?;

        Ok(Self {
            permissions,
            charge: request_charge_from_headers(headers)?,
            activity_id: activity_id_from_headers(headers)?,
            session_token: session_token_from_headers(headers)?,
            content_path: content_path_from_headers(headers)?.to_owned(),
            alt_content_path: alt_content_path_from_headers(headers)?.to_owned(),
            continuation_token: continuation_token_from_headers_optional(headers)?,
        })
    }
}