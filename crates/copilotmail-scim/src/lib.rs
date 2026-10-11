//! SCIM 2.0 HTTP adapter for enterprise Human Principal lifecycle.

#![forbid(unsafe_code)]

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{DateTime, SecondsFormat, Utc};
use copilotmail_application::{
    ControlPlaneStore, DeleteScimGroupCommand, ExistingScimGroup, ExistingScimUser,
    ProvisionScimGroupCommand, ProvisionScimUserCommand, ReplaceScimGroupCommand,
    ReplaceScimUserCommand, ScimCredential, ScimDirectoryQueryStore, ScimGroupService,
    ScimProvisioningError, ScimUserService, StoreError,
};
use copilotmail_domain::{PrincipalId, ScimGroupId};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

const USER_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:User";
const GROUP_SCHEMA: &str = "urn:ietf:params:scim:schemas:core:2.0:Group";
const LIST_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:ListResponse";
const ERROR_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:Error";
const PATCH_SCHEMA: &str = "urn:ietf:params:scim:api:messages:2.0:PatchOp";
const SCIM_CONTENT_TYPE: &str = "application/scim+json";

pub trait ScimBackend:
    ControlPlaneStore + ScimDirectoryQueryStore + Clone + Send + Sync + 'static
{
}
impl<T> ScimBackend for T where
    T: ControlPlaneStore + ScimDirectoryQueryStore + Clone + Send + Sync + 'static
{
}

#[derive(Clone)]
struct ScimState<S> {
    store: S,
    identity_provider: String,
}

/// Builds a self-contained SCIM V2 router whose bearer credentials are
/// independently scoped from human OIDC access tokens.
pub fn router<S: ScimBackend>(store: S, identity_provider: String) -> Router {
    Router::new()
        .route("/ServiceProviderConfig", get(service_provider_config))
        .route("/ResourceTypes", get(resource_types))
        .route("/ResourceTypes/{name}", get(resource_type))
        .route("/Schemas", get(schemas))
        .route("/Schemas/{schema}", get(schema))
        .route("/Users", post(create_user).get(list_users))
        .route(
            "/Users/{principal_id}",
            get(get_user)
                .put(replace_user)
                .patch(patch_user)
                .delete(delete_user),
        )
        .route("/Groups", post(create_group).get(list_groups))
        .route(
            "/Groups/{group_id}",
            get(get_group)
                .put(replace_group)
                .patch(patch_group)
                .delete(delete_group),
        )
        .fallback(scim_not_found)
        .with_state(Arc::new(ScimState {
            store,
            identity_provider,
        }))
}

async fn service_provider_config() -> Response {
    scim_json(
        StatusCode::OK,
        json!({
            "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"],
            "documentationUri": "https://www.rfc-editor.org/rfc/rfc7644",
            "patch": {"supported": true},
            "bulk": {"supported": false, "maxOperations": 0, "maxPayloadSize": 0},
            "filter": {"supported": true, "maxResults": 200},
            "changePassword": {"supported": false},
            "sort": {"supported": false},
            "etag": {"supported": true},
            "authenticationSchemes": [{
                "type": "oauthbearertoken",
                "name": "Tenant-scoped provisioning token",
                "description": "Rotatable CopilotMail SCIM bearer token",
                "specUri": "https://www.rfc-editor.org/rfc/rfc6750",
                "primary": true
            }]
        }),
        None,
    )
}

async fn resource_types() -> Response {
    scim_json(
        StatusCode::OK,
        json!({
            "schemas": [LIST_SCHEMA],
            "totalResults": 2,
            "startIndex": 1,
            "itemsPerPage": 2,
            "Resources": [resource_type_value("User"), resource_type_value("Group")],
        }),
        None,
    )
}

async fn resource_type(Path(name): Path<String>) -> Response {
    match name.as_str() {
        "User" | "Group" => scim_json(StatusCode::OK, resource_type_value(&name), None),
        _ => scim_error(StatusCode::NOT_FOUND, None, "resource type not found"),
    }
}

async fn schemas() -> Response {
    scim_json(
        StatusCode::OK,
        json!({
            "schemas": [LIST_SCHEMA],
            "totalResults": 2,
            "startIndex": 1,
            "itemsPerPage": 2,
            "Resources": [user_schema_value(), group_schema_value()],
        }),
        None,
    )
}

async fn schema(Path(schema): Path<String>) -> Response {
    match schema.as_str() {
        USER_SCHEMA => scim_json(StatusCode::OK, user_schema_value(), None),
        GROUP_SCHEMA => scim_json(StatusCode::OK, group_schema_value(), None),
        _ => scim_error(StatusCode::NOT_FOUND, None, "schema not found"),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserWrite {
    #[serde(default)]
    schemas: Vec<String>,
    external_id: String,
    user_name: String,
    display_name: String,
    #[serde(default = "default_active")]
    active: bool,
}

async fn create_user<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    headers: HeaderMap,
    Json(input): Json<UserWrite>,
) -> Response {
    let request_id = request_id(&headers);
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if let Some(response) = validate_write_headers_and_schema(&headers, &input.schemas) {
        return response;
    }
    let result = ScimUserService::new(state.store.clone())
        .create(ProvisionScimUserCommand {
            credential,
            identity_provider: state.identity_provider.clone(),
            external_id: input.external_id,
            user_name: input.user_name,
            display_name: input.display_name,
            active: input.active,
            trace_id: request_id,
            now: Utc::now(),
        })
        .await;
    match result {
        Ok(result) => {
            let status = if result.created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            user_response(status, &result.user)
        }
        Err(error) => provisioning_error(error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListQuery {
    filter: Option<String>,
    start_index: Option<i64>,
    count: Option<i64>,
}

async fn list_users<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (external_id, user_name) = match parse_filter(query.filter.as_deref()) {
        Ok(value) => value,
        Err(()) => {
            return scim_error(
                StatusCode::BAD_REQUEST,
                Some("invalidFilter"),
                "unsupported filter",
            );
        }
    };
    let start = query.start_index.unwrap_or(1).max(1);
    let count = query.count.unwrap_or(100).clamp(0, 200);
    let result = state
        .store
        .list_scim_users(
            credential.organization_id,
            external_id.as_deref(),
            user_name.as_deref(),
            start - 1,
            count,
        )
        .await;
    match result {
        Ok((users, total)) => {
            let resources = users.iter().map(user_value).collect::<Vec<_>>();
            scim_json(
                StatusCode::OK,
                json!({
                    "schemas": [LIST_SCHEMA],
                    "totalResults": total,
                    "startIndex": start,
                    "itemsPerPage": resources.len(),
                    "Resources": resources,
                }),
                None,
            )
        }
        Err(_) => scim_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            "directory store is unavailable",
        ),
    }
}

async fn get_user<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(principal_id): Path<PrincipalId>,
    headers: HeaderMap,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state
        .store
        .get_scim_user(credential.organization_id, principal_id)
        .await
    {
        Ok(Some(user)) => user_response(StatusCode::OK, &user),
        Ok(None) => scim_error(StatusCode::NOT_FOUND, None, "resource not found"),
        Err(_) => scim_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            "directory store is unavailable",
        ),
    }
}

async fn replace_user<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(principal_id): Path<PrincipalId>,
    headers: HeaderMap,
    Json(input): Json<UserWrite>,
) -> Response {
    let request_id = request_id(&headers);
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if let Some(response) = validate_write_headers_and_schema(&headers, &input.schemas) {
        return response;
    }
    replace(
        &state.store,
        ReplaceScimUserCommand {
            credential,
            principal_id,
            external_id: input.external_id,
            user_name: input.user_name,
            display_name: input.display_name,
            active: input.active,
            expected_version: if_match(&headers),
            trace_id: request_id,
            now: Utc::now(),
        },
    )
    .await
}

#[derive(Debug, Deserialize)]
struct PatchRequest {
    schemas: Vec<String>,
    #[serde(rename = "Operations")]
    operations: Vec<PatchOperation>,
}

#[derive(Debug, Deserialize)]
struct PatchOperation {
    op: String,
    path: Option<String>,
    value: Value,
}

async fn patch_user<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(principal_id): Path<PrincipalId>,
    headers: HeaderMap,
    Json(input): Json<PatchRequest>,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !content_type_is_scim(&headers)
        || input.schemas.as_slice() != [PATCH_SCHEMA]
        || input.operations.is_empty()
        || input.operations.len() > 20
    {
        return scim_error(
            StatusCode::BAD_REQUEST,
            Some("invalidSyntax"),
            "invalid PATCH request",
        );
    }
    let current = match state
        .store
        .get_scim_user(credential.organization_id, principal_id)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return scim_error(StatusCode::NOT_FOUND, None, "resource not found"),
        Err(_) => {
            return scim_error(
                StatusCode::SERVICE_UNAVAILABLE,
                None,
                "directory store is unavailable",
            );
        }
    };
    let mut external_id = current.external_id;
    let mut user_name = current.user_name;
    let mut display_name = current.display_name;
    let mut active = current.active;
    for operation in input.operations {
        if !operation.op.eq_ignore_ascii_case("replace") {
            return scim_error(
                StatusCode::BAD_REQUEST,
                Some("invalidSyntax"),
                "only replace is supported",
            );
        }
        match operation
            .path
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("username") => match operation.value.as_str() {
                Some(value) => user_name = value.into(),
                None => return invalid_patch_value(),
            },
            Some("displayname") => match operation.value.as_str() {
                Some(value) => display_name = value.into(),
                None => return invalid_patch_value(),
            },
            Some("active") => match operation.value.as_bool() {
                Some(value) => active = value,
                None => return invalid_patch_value(),
            },
            Some("externalid") => {
                return scim_error(
                    StatusCode::BAD_REQUEST,
                    Some("mutability"),
                    "externalId is immutable",
                );
            }
            None => {
                let Some(value) = operation.value.as_object() else {
                    return invalid_patch_value();
                };
                if let Some(value) = value.get("userName").and_then(Value::as_str) {
                    user_name = value.into();
                }
                if let Some(value) = value.get("displayName").and_then(Value::as_str) {
                    display_name = value.into();
                }
                if let Some(value) = value.get("active").and_then(Value::as_bool) {
                    active = value;
                }
                if value.contains_key("externalId") {
                    return scim_error(
                        StatusCode::BAD_REQUEST,
                        Some("mutability"),
                        "externalId is immutable",
                    );
                }
            }
            _ => {
                return scim_error(
                    StatusCode::BAD_REQUEST,
                    Some("invalidPath"),
                    "unsupported PATCH path",
                );
            }
        }
    }
    replace(
        &state.store,
        ReplaceScimUserCommand {
            credential,
            principal_id,
            external_id: std::mem::take(&mut external_id),
            user_name,
            display_name,
            active,
            expected_version: if_match(&headers),
            trace_id: request_id(&headers),
            now: Utc::now(),
        },
    )
    .await
}

async fn delete_user<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(principal_id): Path<PrincipalId>,
    headers: HeaderMap,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let current = match state
        .store
        .get_scim_user(credential.organization_id, principal_id)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return scim_error(StatusCode::NOT_FOUND, None, "resource not found"),
        Err(_) => {
            return scim_error(
                StatusCode::SERVICE_UNAVAILABLE,
                None,
                "directory store is unavailable",
            );
        }
    };
    match ScimUserService::new(state.store.clone())
        .replace(ReplaceScimUserCommand {
            credential,
            principal_id,
            external_id: current.external_id,
            user_name: current.user_name,
            display_name: current.display_name,
            active: false,
            expected_version: if_match(&headers),
            trace_id: request_id(&headers),
            now: Utc::now(),
        })
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => provisioning_error(error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupWrite {
    #[serde(default)]
    schemas: Vec<String>,
    external_id: String,
    display_name: String,
    #[serde(default)]
    members: Vec<MemberWrite>,
}

#[derive(Debug, Deserialize)]
struct MemberWrite {
    value: String,
}

async fn create_group<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    headers: HeaderMap,
    Json(input): Json<GroupWrite>,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if let Some(response) = validate_group_headers_and_schema(&headers, &input.schemas) {
        return response;
    }
    let members = match member_ids(input.members) {
        Ok(value) => value,
        Err(()) => return invalid_patch_value(),
    };
    match ScimGroupService::new(state.store.clone())
        .create(ProvisionScimGroupCommand {
            credential,
            external_id: input.external_id,
            display_name: input.display_name,
            members,
            trace_id: request_id(&headers),
            now: Utc::now(),
        })
        .await
    {
        Ok(result) => group_response(
            if result.created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            },
            &result.group,
        ),
        Err(error) => provisioning_error(error),
    }
}

async fn list_groups<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let (external_id, display_name) = match parse_group_filter(query.filter.as_deref()) {
        Ok(value) => value,
        Err(()) => {
            return scim_error(
                StatusCode::BAD_REQUEST,
                Some("invalidFilter"),
                "unsupported filter",
            );
        }
    };
    let start = query.start_index.unwrap_or(1).max(1);
    let count = query.count.unwrap_or(100).clamp(0, 200);
    match state
        .store
        .list_scim_groups(
            credential.organization_id,
            external_id.as_deref(),
            display_name.as_deref(),
            start - 1,
            count,
        )
        .await
    {
        Ok((groups, total)) => {
            let resources = groups.iter().map(group_value).collect::<Vec<_>>();
            scim_json(
                StatusCode::OK,
                json!({
                    "schemas": [LIST_SCHEMA],
                    "totalResults": total,
                    "startIndex": start,
                    "itemsPerPage": resources.len(),
                    "Resources": resources,
                }),
                None,
            )
        }
        Err(_) => scim_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            "directory store is unavailable",
        ),
    }
}

async fn get_group<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(group_id): Path<ScimGroupId>,
    headers: HeaderMap,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match state
        .store
        .get_scim_group(credential.organization_id, group_id)
        .await
    {
        Ok(Some(group)) => group_response(StatusCode::OK, &group),
        Ok(None) => scim_error(StatusCode::NOT_FOUND, None, "resource not found"),
        Err(_) => scim_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            "directory store is unavailable",
        ),
    }
}

async fn replace_group<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(group_id): Path<ScimGroupId>,
    headers: HeaderMap,
    Json(input): Json<GroupWrite>,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if let Some(response) = validate_group_headers_and_schema(&headers, &input.schemas) {
        return response;
    }
    let members = match member_ids(input.members) {
        Ok(value) => value,
        Err(()) => return invalid_patch_value(),
    };
    replace_group_command(
        &state.store,
        ReplaceScimGroupCommand {
            credential,
            group_id,
            external_id: input.external_id,
            display_name: input.display_name,
            members,
            expected_version: if_match(&headers),
            trace_id: request_id(&headers),
            now: Utc::now(),
        },
    )
    .await
}

async fn patch_group<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(group_id): Path<ScimGroupId>,
    headers: HeaderMap,
    Json(input): Json<PatchRequest>,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    if !content_type_is_scim(&headers)
        || input.schemas.as_slice() != [PATCH_SCHEMA]
        || input.operations.is_empty()
        || input.operations.len() > 100
    {
        return scim_error(
            StatusCode::BAD_REQUEST,
            Some("invalidSyntax"),
            "invalid PATCH request",
        );
    }
    let current = match state
        .store
        .get_scim_group(credential.organization_id, group_id)
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return scim_error(StatusCode::NOT_FOUND, None, "resource not found"),
        Err(_) => {
            return scim_error(
                StatusCode::SERVICE_UNAVAILABLE,
                None,
                "directory store is unavailable",
            );
        }
    };
    let external_id = current.external_id;
    let mut display_name = current.display_name;
    let mut members = current.members;
    for operation in input.operations {
        let op = operation.op.to_ascii_lowercase();
        let path = operation.path.as_deref().unwrap_or("");
        match (op.as_str(), path.to_ascii_lowercase().as_str()) {
            ("replace", "displayname") => match operation.value.as_str() {
                Some(value) => display_name = value.into(),
                None => return invalid_patch_value(),
            },
            ("replace", "members") => match member_ids_value(&operation.value) {
                Ok(value) => members = value,
                Err(()) => return invalid_patch_value(),
            },
            ("add", "members") => match member_ids_value(&operation.value) {
                Ok(value) => members.extend(value),
                Err(()) => return invalid_patch_value(),
            },
            ("remove", "members") => members.clear(),
            ("remove", path) if path.starts_with("members[value eq \"") => {
                let Some(value) = path
                    .strip_prefix("members[value eq \"")
                    .and_then(|value| value.strip_suffix("\"]"))
                else {
                    return invalid_patch_path();
                };
                let Ok(principal_id) = value.parse::<PrincipalId>() else {
                    return invalid_patch_value();
                };
                members.retain(|member| *member != principal_id);
            }
            ("replace" | "add", "") => {
                let Some(value) = operation.value.as_object() else {
                    return invalid_patch_value();
                };
                if let Some(value) = value.get("displayName").and_then(Value::as_str) {
                    display_name = value.into();
                }
                if let Some(value) = value.get("members") {
                    let new_members = match member_ids_value(value) {
                        Ok(value) => value,
                        Err(()) => return invalid_patch_value(),
                    };
                    if op == "add" {
                        members.extend(new_members);
                    } else {
                        members = new_members;
                    }
                }
                if value.contains_key("externalId") {
                    return mutability_error();
                }
            }
            (_, "externalid") => return mutability_error(),
            _ => return invalid_patch_path(),
        }
    }
    replace_group_command(
        &state.store,
        ReplaceScimGroupCommand {
            credential,
            group_id,
            external_id,
            display_name,
            members,
            expected_version: if_match(&headers),
            trace_id: request_id(&headers),
            now: Utc::now(),
        },
    )
    .await
}

async fn delete_group<S: ScimBackend>(
    State(state): State<Arc<ScimState<S>>>,
    Path(group_id): Path<ScimGroupId>,
    headers: HeaderMap,
) -> Response {
    let credential = match authenticate(&state.store, &headers).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    match ScimGroupService::new(state.store.clone())
        .delete(DeleteScimGroupCommand {
            credential,
            group_id,
            expected_version: if_match(&headers),
            trace_id: request_id(&headers),
            now: Utc::now(),
        })
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => provisioning_error(error),
    }
}

async fn replace_group_command<S: ScimBackend>(
    store: &S,
    command: ReplaceScimGroupCommand,
) -> Response {
    match ScimGroupService::new(store.clone()).replace(command).await {
        Ok(group) => group_response(StatusCode::OK, &group),
        Err(error) => provisioning_error(error),
    }
}

async fn replace<S: ScimBackend>(store: &S, command: ReplaceScimUserCommand) -> Response {
    match ScimUserService::new(store.clone()).replace(command).await {
        Ok(user) => user_response(StatusCode::OK, &user),
        Err(error) => provisioning_error(error),
    }
}

async fn authenticate<S: ScimDirectoryQueryStore>(
    store: &S,
    headers: &HeaderMap,
) -> Result<ScimCredential, Response> {
    let Some(token) = bearer_token(headers) else {
        return Err(unauthorized());
    };
    let hash = Sha256::digest(token.as_bytes());
    match store
        .authenticate_scim_token(hash.as_slice(), Utc::now())
        .await
    {
        Ok(Some(credential)) => Ok(credential),
        Ok(None) => Err(unauthorized()),
        Err(_) => Err(scim_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            "credential verification is unavailable",
        )),
    }
}

fn validate_write_headers_and_schema(headers: &HeaderMap, schemas: &[String]) -> Option<Response> {
    if !content_type_is_scim(headers) {
        return Some(scim_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Some("invalidSyntax"),
            "Content-Type must be application/scim+json",
        ));
    }
    if schemas != [USER_SCHEMA] {
        return Some(scim_error(
            StatusCode::BAD_REQUEST,
            Some("invalidSyntax"),
            "the core User schema is required",
        ));
    }
    None
}

fn validate_group_headers_and_schema(headers: &HeaderMap, schemas: &[String]) -> Option<Response> {
    if !content_type_is_scim(headers) {
        return Some(scim_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Some("invalidSyntax"),
            "Content-Type must be application/scim+json",
        ));
    }
    if schemas != [GROUP_SCHEMA] {
        return Some(scim_error(
            StatusCode::BAD_REQUEST,
            Some("invalidSyntax"),
            "the core Group schema is required",
        ));
    }
    None
}

fn parse_filter(filter: Option<&str>) -> Result<(Option<String>, Option<String>), ()> {
    let Some(filter) = filter else {
        return Ok((None, None));
    };
    let (attribute, value) = filter.split_once(" eq ").ok_or(())?;
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err(());
    }
    let value = &value[1..value.len() - 1];
    if value.contains(['"', '\\']) {
        return Err(());
    }
    match attribute.to_ascii_lowercase().as_str() {
        "externalid" => Ok((Some(value.into()), None)),
        "username" => Ok((None, Some(value.into()))),
        _ => Err(()),
    }
}

fn parse_group_filter(filter: Option<&str>) -> Result<(Option<String>, Option<String>), ()> {
    let Some(filter) = filter else {
        return Ok((None, None));
    };
    let (attribute, value) = filter.split_once(" eq ").ok_or(())?;
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err(());
    }
    let value = &value[1..value.len() - 1];
    if value.contains(['"', '\\']) {
        return Err(());
    }
    match attribute.to_ascii_lowercase().as_str() {
        "externalid" => Ok((Some(value.into()), None)),
        "displayname" => Ok((None, Some(value.into()))),
        _ => Err(()),
    }
}

fn member_ids(members: Vec<MemberWrite>) -> Result<Vec<PrincipalId>, ()> {
    members
        .into_iter()
        .map(|member| member.value.parse().map_err(|_| ()))
        .collect()
}

fn member_ids_value(value: &Value) -> Result<Vec<PrincipalId>, ()> {
    let Some(members) = value.as_array() else {
        return Err(());
    };
    members
        .iter()
        .map(|member| {
            member
                .get("value")
                .and_then(Value::as_str)
                .ok_or(())?
                .parse()
                .map_err(|_| ())
        })
        .collect()
}

fn user_response(status: StatusCode, user: &ExistingScimUser) -> Response {
    scim_json(status, user_value(user), Some(user))
}

fn resource_type_value(name: &str) -> Value {
    let (endpoint, schema) = if name == "User" {
        ("/Users", USER_SCHEMA)
    } else {
        ("/Groups", GROUP_SCHEMA)
    };
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:ResourceType"],
        "id": name,
        "name": name,
        "endpoint": endpoint,
        "schema": schema,
        "schemaExtensions": [],
        "meta": {
            "resourceType": "ResourceType",
            "location": format!("/scim/v2/ResourceTypes/{name}")
        }
    })
}

fn user_schema_value() -> Value {
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": USER_SCHEMA,
        "name": "User",
        "description": "CopilotMail enterprise human identity",
        "attributes": [
            schema_attribute("userName", "string", true, "readWrite", "server"),
            schema_attribute("externalId", "string", true, "immutable", "server"),
            schema_attribute("displayName", "string", false, "readWrite", "none"),
            schema_attribute("active", "boolean", false, "readWrite", "none")
        ],
        "meta": {
            "resourceType": "Schema",
            "location": format!("/scim/v2/Schemas/{USER_SCHEMA}")
        }
    })
}

fn group_schema_value() -> Value {
    let mut members = schema_attribute("members", "complex", false, "readWrite", "none");
    members["multiValued"] = json!(true);
    members["subAttributes"] = json!([
        schema_attribute("value", "string", false, "immutable", "none"),
        schema_attribute("$ref", "reference", false, "immutable", "none"),
        schema_attribute("type", "string", false, "immutable", "none")
    ]);
    json!({
        "schemas": ["urn:ietf:params:scim:schemas:core:2.0:Schema"],
        "id": GROUP_SCHEMA,
        "name": "Group",
        "description": "CopilotMail enterprise directory group",
        "attributes": [
            schema_attribute("displayName", "string", true, "readWrite", "server"),
            schema_attribute("externalId", "string", true, "immutable", "server"),
            members
        ],
        "meta": {
            "resourceType": "Schema",
            "location": format!("/scim/v2/Schemas/{GROUP_SCHEMA}")
        }
    })
}

fn schema_attribute(
    name: &str,
    kind: &str,
    required: bool,
    mutability: &str,
    uniqueness: &str,
) -> Value {
    json!({
        "name": name,
        "type": kind,
        "multiValued": false,
        "description": name,
        "required": required,
        "caseExact": name == "externalId",
        "mutability": mutability,
        "returned": "default",
        "uniqueness": uniqueness
    })
}

fn user_value(user: &ExistingScimUser) -> Value {
    json!({
        "schemas": [USER_SCHEMA],
        "id": user.principal_id,
        "externalId": user.external_id,
        "userName": user.user_name,
        "displayName": user.display_name,
        "active": user.active,
        "meta": {
            "resourceType": "User",
            "created": timestamp(user.created_at),
            "lastModified": timestamp(user.updated_at),
            "version": etag(user.version),
            "location": format!("/scim/v2/Users/{}", user.principal_id),
        }
    })
}

fn group_response(status: StatusCode, group: &ExistingScimGroup) -> Response {
    let mut response = scim_json(status, group_value(group), None);
    response.headers_mut().insert(
        header::ETAG,
        HeaderValue::from_str(&etag(group.version)).expect("numeric ETag is valid"),
    );
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&format!("/scim/v2/Groups/{}", group.group_id))
            .expect("UUID location is valid"),
    );
    response
}

fn group_value(group: &ExistingScimGroup) -> Value {
    json!({
        "schemas": [GROUP_SCHEMA],
        "id": group.group_id,
        "externalId": group.external_id,
        "displayName": group.display_name,
        "members": group.members.iter().map(|member| json!({
            "value": member,
            "$ref": format!("/scim/v2/Users/{member}"),
            "type": "User",
        })).collect::<Vec<_>>(),
        "meta": {
            "resourceType": "Group",
            "created": timestamp(group.created_at),
            "lastModified": timestamp(group.updated_at),
            "version": etag(group.version),
            "location": format!("/scim/v2/Groups/{}", group.group_id),
        }
    })
}

fn scim_json(status: StatusCode, value: Value, user: Option<&ExistingScimUser>) -> Response {
    let mut response = (status, Json(value)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(SCIM_CONTENT_TYPE),
    );
    if let Some(user) = user {
        response.headers_mut().insert(
            header::ETAG,
            HeaderValue::from_str(&etag(user.version)).expect("numeric ETag is valid"),
        );
        response.headers_mut().insert(
            header::LOCATION,
            HeaderValue::from_str(&format!("/scim/v2/Users/{}", user.principal_id))
                .expect("UUID location is valid"),
        );
    }
    response
}

fn provisioning_error(error: ScimProvisioningError) -> Response {
    match error {
        ScimProvisioningError::InvalidAttribute(attribute) => scim_error(
            StatusCode::BAD_REQUEST,
            Some("invalidValue"),
            &format!("invalid {attribute}"),
        ),
        ScimProvisioningError::VersionMismatch => scim_error(
            StatusCode::PRECONDITION_FAILED,
            None,
            "resource version does not match",
        ),
        ScimProvisioningError::Store(StoreError::Conflict) => scim_error(
            StatusCode::CONFLICT,
            Some("uniqueness"),
            "resource conflicts with an existing identity",
        ),
        ScimProvisioningError::Store(StoreError::MissingReference) => {
            scim_error(StatusCode::NOT_FOUND, None, "resource not found")
        }
        ScimProvisioningError::Store(
            StoreError::Unavailable
            | StoreError::AgentMailAdmission(_)
            | StoreError::AuthenticationInvalid,
        ) => scim_error(
            StatusCode::SERVICE_UNAVAILABLE,
            None,
            "directory store is unavailable",
        ),
        ScimProvisioningError::Domain(_) => scim_error(
            StatusCode::BAD_REQUEST,
            Some("invalidValue"),
            "identity attributes are invalid",
        ),
    }
}

fn scim_error(status: StatusCode, scim_type: Option<&str>, detail: &str) -> Response {
    let mut error = json!({
        "schemas": [ERROR_SCHEMA],
        "status": status.as_u16().to_string(),
        "detail": detail,
    });
    if let Some(scim_type) = scim_type {
        error["scimType"] = json!(scim_type);
    }
    scim_json(status, error, None)
}

async fn scim_not_found() -> Response {
    scim_error(StatusCode::NOT_FOUND, None, "SCIM route not found")
}

fn unauthorized() -> Response {
    let mut response = scim_error(
        StatusCode::UNAUTHORIZED,
        None,
        "a valid SCIM bearer token is required",
    );
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Bearer realm=\"CopilotMail SCIM\""),
    );
    response
}

fn invalid_patch_value() -> Response {
    scim_error(
        StatusCode::BAD_REQUEST,
        Some("invalidValue"),
        "PATCH value has the wrong type",
    )
}

fn invalid_patch_path() -> Response {
    scim_error(
        StatusCode::BAD_REQUEST,
        Some("invalidPath"),
        "unsupported PATCH path",
    )
}

fn mutability_error() -> Response {
    scim_error(
        StatusCode::BAD_REQUEST,
        Some("mutability"),
        "externalId is immutable",
    )
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    (!token.is_empty() && !token.bytes().any(|byte| byte.is_ascii_whitespace())).then_some(token)
}

fn content_type_is_scim(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.eq_ignore_ascii_case(SCIM_CONTENT_TYPE))
}

fn if_match(headers: &HeaderMap) -> Option<i64> {
    let value = headers.get(header::IF_MATCH)?;
    Some(
        value
            .to_str()
            .ok()
            .and_then(|value| value.strip_prefix("W/\""))
            .and_then(|value| value.strip_suffix('"'))
            .and_then(|value| value.parse().ok())
            .unwrap_or(i64::MIN),
    )
}

fn etag(version: i64) -> String {
    format!("W/\"{version}\"")
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn request_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 200)
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::now_v7().to_string())
}

const fn default_active() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use copilotmail_application::{ControlPlaneTransaction, StoreError};
    use copilotmail_domain::OrganizationId;
    use tower::ServiceExt;

    #[test]
    fn filter_parser_is_deliberately_bounded() {
        assert_eq!(
            parse_filter(Some(r#"userName eq "ada@example.test""#)).unwrap(),
            (None, Some("ada@example.test".into()))
        );
        assert!(parse_filter(Some(r#"userName co "ada""#)).is_err());
        assert!(parse_filter(Some(r#"emails.value eq "ada@example.test""#)).is_err());
    }

    #[test]
    fn weak_etags_round_trip() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_MATCH, HeaderValue::from_static("W/\"42\""));
        assert_eq!(if_match(&headers), Some(42));
        assert_eq!(etag(42), "W/\"42\"");
        headers.insert(header::IF_MATCH, HeaderValue::from_static("broken"));
        assert_eq!(if_match(&headers), Some(i64::MIN));
    }

    #[derive(Clone)]
    struct NoCredentials;

    #[async_trait]
    impl ControlPlaneStore for NoCredentials {
        async fn begin(&self) -> Result<Box<dyn ControlPlaneTransaction>, StoreError> {
            Err(StoreError::Unavailable)
        }
    }

    #[async_trait]
    impl ScimDirectoryQueryStore for NoCredentials {
        async fn authenticate_scim_token(
            &self,
            _: &[u8],
            _: DateTime<Utc>,
        ) -> Result<Option<ScimCredential>, StoreError> {
            Ok(None)
        }

        async fn get_scim_user(
            &self,
            _: OrganizationId,
            _: PrincipalId,
        ) -> Result<Option<ExistingScimUser>, StoreError> {
            Ok(None)
        }

        async fn list_scim_users(
            &self,
            _: OrganizationId,
            _: Option<&str>,
            _: Option<&str>,
            _: i64,
            _: i64,
        ) -> Result<(Vec<ExistingScimUser>, i64), StoreError> {
            Ok((Vec::new(), 0))
        }

        async fn get_scim_group(
            &self,
            _: OrganizationId,
            _: ScimGroupId,
        ) -> Result<Option<ExistingScimGroup>, StoreError> {
            Ok(None)
        }

        async fn list_scim_groups(
            &self,
            _: OrganizationId,
            _: Option<&str>,
            _: Option<&str>,
            _: i64,
            _: i64,
        ) -> Result<(Vec<ExistingScimGroup>, i64), StoreError> {
            Ok((Vec::new(), 0))
        }
    }

    #[tokio::test]
    async fn discovery_uses_scim_media_type_and_declares_supported_features() {
        let response = router(NoCredentials, "https://login.example.test".into())
            .oneshot(
                Request::builder()
                    .uri("/ServiceProviderConfig")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], SCIM_CONTENT_TYPE);
        let body = to_bytes(response.into_body(), 32 * 1024).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["patch"]["supported"], true);
        assert_eq!(value["bulk"]["supported"], false);
        assert_eq!(value["etag"]["supported"], true);
    }

    #[tokio::test]
    async fn directory_resources_fail_closed_without_scim_credential() {
        let response = router(NoCredentials, "https://login.example.test".into())
            .oneshot(
                Request::builder()
                    .uri("/Users")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().contains_key(header::WWW_AUTHENTICATE));
        assert_eq!(response.headers()[header::CONTENT_TYPE], SCIM_CONTENT_TYPE);
    }
}
