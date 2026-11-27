use std::sync::Arc;

use base64::Engine as _;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use synra_core::transfer::upload::UploadError;
use synra_ipc::dto::{
    PluginUploadAppend, PluginUploadBegin, PluginUploadChunk, PluginUploadEnded,
    PluginUploadHandle, PluginUploadOpened, PluginUploadSent,
};
use synra_protocol::DeviceId;
use uuid::Uuid;

use super::super::{
    AppState, ErrorCode, IpcError, ValidatedSession, authority, internal, require_grant,
};
use super::uploads::{Failure, OpenUpload};
use crate::commands::transfers::{self, SendSource};

const SEND_GRANT: &str = "file:transfer:send";

pub(crate) fn upload_time_ms(state: &AppState) -> u64 {
    u64::try_from(state.uptime().as_millis()).unwrap_or(u64::MAX)
}

fn request<T: DeserializeOwned>(args: &Value) -> Result<T, IpcError> {
    T::deserialize(args).map_err(|error| invalid("upload", error))
}

fn invalid(field: &str, reason: impl std::fmt::Display) -> IpcError {
    IpcError::unknown(ErrorCode::InvalidArgument)
        .with_params(json!({"field": field, "reason": reason.to_string()}))
}

fn reply(value: impl Serialize) -> Result<Value, IpcError> {
    serde_json::to_value(value).map_err(internal)
}

fn failure(error: Failure, field: &str, id: Option<Uuid>) -> IpcError {
    let (code, params) = match error {
        Failure::Admission(error) => return error,
        Failure::Io(error) => {
            eprintln!("synra: upload {id:?} failed on disk: {error}");
            (ErrorCode::Internal, json!({"reason":"upload_io"}))
        }
        Failure::Rule(rule) => match rule {
            UploadError::Empty => (
                ErrorCode::InvalidArgument,
                json!({"field": field, "reason": "upload_empty"}),
            ),
            UploadError::Size { limit } => (
                ErrorCode::PayloadTooLarge,
                json!({"field": field, "maxBytes": limit}),
            ),
            UploadError::Capacity => (ErrorCode::RateLimited, json!({"reason": "upload_capacity"})),
            UploadError::Offset { expected } => (
                ErrorCode::InvalidArgument,
                json!({"field": "offset", "expectedOffset": expected}),
            ),
            UploadError::Incomplete { expected } => (
                ErrorCode::InvalidArgument,
                json!({"field": "uploadId", "totalBytes": expected, "reason": "upload_incomplete"}),
            ),
            UploadError::Closed => (
                ErrorCode::InvalidArgument,
                json!({"field": "uploadId", "reason": "upload_not_found"}),
            ),
        },
    };
    IpcError::unknown(code).with_params(params)
}

fn upload_id(text: &str) -> Result<Uuid, IpcError> {
    text.parse().map_err(|error| invalid("uploadId", error))
}

fn file_name(name: &str) -> Result<(), IpcError> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|ch| ch.is_control() || "/\\:<>\"|?*".contains(ch))
    {
        return Err(invalid(
            "fileName",
            "the name must be one portable path component",
        ));
    }
    Ok(())
}

pub(crate) fn transfer_begin(
    state: &AppState,
    session: &ValidatedSession,
    args: &Value,
) -> Result<Value, IpcError> {
    require_grant(session, SEND_GRANT, "transfer.begin")?;
    let args: PluginUploadBegin = request(args)?;
    let to = args
        .to
        .parse::<DeviceId>()
        .map_err(|error| invalid("to", error))?;
    file_name(&args.file_name)?;
    let (id, chunk_bytes) = state
        .uploads
        .begin(
            &state.paths.cache_dir(),
            OpenUpload {
                owner: session.into(),
                to,
                file_name: &args.file_name,
                total: args.total_bytes,
            },
            upload_time_ms(state),
            || {
                state
                    .ui_sessions
                    .validate(
                        session.nonce,
                        session.plugin_id(),
                        &session.token().plugin_version,
                        transfers::unix_millis(),
                    )
                    .is_ok()
            },
        )
        .map_err(|error| failure(error, "totalBytes", None))?;
    reply(PluginUploadOpened {
        upload_id: id.to_string(),
        chunk_bytes,
    })
}

pub(crate) fn transfer_chunk(
    state: &AppState,
    session: &ValidatedSession,
    args: &Value,
) -> Result<Value, IpcError> {
    require_grant(session, SEND_GRANT, "transfer.chunk")?;
    let limit = state.uploads.chunk_limit();
    if args
        .get("data")
        .and_then(Value::as_str)
        .is_some_and(|data| data.len() > limit.div_ceil(3) * 4)
    {
        return Err(failure(
            UploadError::Size {
                limit: limit as u64,
            }
            .into(),
            "data",
            None,
        ));
    }
    let args: PluginUploadChunk = request(args)?;
    let id = upload_id(&args.upload_id)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(args.data)
        .map_err(|error| invalid("data", error))?;
    let (written, total_bytes) = state
        .uploads
        .append(
            &session.into(),
            id,
            args.offset,
            &bytes,
            upload_time_ms(state),
        )
        .map_err(|error| failure(error, "data", Some(id)))?;
    reply(PluginUploadAppend {
        written,
        total_bytes,
    })
}

pub(crate) fn transfer_finish(
    state: &Arc<AppState>,
    session: &ValidatedSession,
    args: &Value,
) -> Result<Value, IpcError> {
    require_grant(session, SEND_GRANT, "transfer.finish")?;
    let args: PluginUploadHandle = request(args)?;
    let id = upload_id(&args.upload_id)?;
    let transfer_id = state
        .uploads
        .finish(&session.into(), id, upload_time_ms(state), |to, spool| {
            let (address, sender, row) =
                transfers::prepare_file_send(state, to, &spool.path().to_string_lossy())?;
            let entry = state
                .hosted_ui
                .lock()
                .values()
                .find(|entry| entry.nonce == session.nonce)
                .cloned();
            let entry = entry.ok_or_else(authority::refused)?;
            let live = authority::validate(state, &entry)?;
            require_grant(&live, SEND_GRANT, "transfer.finish")?;
            transfers::start_send(
                state,
                to,
                address,
                sender,
                row,
                SendSource::PluginUpload {
                    owner: live.plugin_id().clone(),
                    spool,
                },
            )
        })
        .map_err(|error| failure(error, "uploadId", Some(id)))?;
    reply(PluginUploadSent { transfer_id })
}

pub(crate) fn transfer_abort(
    state: &AppState,
    session: &ValidatedSession,
    args: &Value,
) -> Result<Value, IpcError> {
    require_grant(session, SEND_GRANT, "transfer.abort")?;
    let args: PluginUploadHandle = request(args)?;
    let id = upload_id(&args.upload_id)?;
    reply(PluginUploadEnded {
        aborted: state.uploads.abort(&session.into(), id),
    })
}
