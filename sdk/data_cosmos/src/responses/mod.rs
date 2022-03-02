//! Responses from any call to the Cosmos API.

#![allow(missing_docs)]

mod create_collection_response;
mod create_reference_attachment_response;
mod create_slug_attachment_response;
mod create_trigger_response;
mod create_user_defined_function_response;
mod delete_attachment_response;
mod delete_trigger_response;
mod delete_user_defined_function_response;
mod execute_stored_procedure_response;
mod get_attachment_response;
mod get_partition_key_ranges_response;
mod list_stored_procedures_response;
mod list_triggers_response;
mod list_user_defined_functions_response;
mod query_documents_response;
mod replace_reference_attachment_response;

pub use create_collection_response::CreateCollectionResponse;
pub use create_reference_attachment_response::CreateReferenceAttachmentResponse;
pub use create_slug_attachment_response::CreateSlugAttachmentResponse;
pub use create_trigger_response::CreateTriggerResponse;
pub use create_user_defined_function_response::CreateUserDefinedFunctionResponse;
pub use delete_attachment_response::DeleteAttachmentResponse;
pub use delete_trigger_response::DeleteTriggerResponse;
pub use delete_user_defined_function_response::DeleteUserDefinedFunctionResponse;
pub use execute_stored_procedure_response::ExecuteStoredProcedureResponse;
pub use get_attachment_response::GetAttachmentResponse;
pub use get_partition_key_ranges_response::GetPartitionKeyRangesResponse;
pub use list_stored_procedures_response::ListStoredProceduresResponse;
pub use list_triggers_response::ListTriggersResponse;
pub use list_user_defined_functions_response::ListUserDefinedFunctionsResponse;
pub use query_documents_response::{
    QueryDocumentsResponse, QueryDocumentsResponseDocuments, QueryDocumentsResponseRaw,
    QueryResponseMeta, QueryResult,
};
pub use replace_reference_attachment_response::ReplaceReferenceAttachmentResponse;
