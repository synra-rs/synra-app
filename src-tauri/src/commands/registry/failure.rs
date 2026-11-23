//! Transport refusals keep their network classification and never become configuration problems.

use synra_ipc::IpcError;
use synra_protocol::{ErrorCode, error::SuggestedAction};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Operation {
    Search,
    Metadata,
    Download,
}

impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Metadata => "metadata",
            Self::Download => "download",
        }
    }
}

pub(crate) fn http_failure(status: u16, url: &str, operation: Operation) -> IpcError {
    let error = match (status, operation) {
        (404, Operation::Search) => synra_ipc::RegistryProblem::SearchUnsupported.into_error(),
        (408, _) => IpcError::unknown(ErrorCode::Timeout),
        (429, _) => IpcError::unknown(ErrorCode::RateLimited),
        (500..=599, _) => IpcError::unknown(ErrorCode::Internal),
        _ => IpcError::unknown(ErrorCode::InvalidArgument),
    };
    offer_retry(error.with_params(serde_json::json!({
        "url": url, "status": status, "reason": format!("the registry {} refused the request", operation.label()),
    })))
}

pub(crate) fn network_failure(refusal: synra_core::network::Refusal, url: &str) -> IpcError {
    offer_retry(
        IpcError::unknown(refusal.code).with_params(serde_json::json!({
            "url": url, "reason": refusal.message,
        })),
    )
}

pub(crate) fn exhausted(last: Option<IpcError>, operation: Operation) -> IpcError {
    last.unwrap_or_else(|| {
        IpcError::unknown(ErrorCode::Internal).with_params(serde_json::json!({
            "reason": format!("no registry request was attempted for {}", operation.label()),
        }))
    })
}

fn offer_retry(error: IpcError) -> IpcError {
    if error.retryable {
        error.with_action(SuggestedAction::Retry)
    } else {
        error
    }
}
