// QCue S1-R48 (dispatch seam) — the ONE provider seam. The turn loop calls `dispatch.complete`
// and never branches on provider/api_mode. Stub & real HTTP both implement `ProviderDispatch`.
use crate::stub::StubProvider;
use crate::transport::ReqParams;
use async_trait::async_trait;
use protocol::{ApiError, Message, NormalizedResponse, ToolDef};
use tokio_util::sync::CancellationToken;

/// One model round-trip's inputs. The route (provider/model/api_mode) lives inside the
/// dispatch impl, NOT here — keeps the loop and TurnContext provider-agnostic.
pub struct DispatchRequest {
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDef>,
    pub params: ReqParams,
    pub tenant: uuid::Uuid,
}

#[async_trait]
pub trait ProviderDispatch: Send + Sync {
    async fn complete(
        &self,
        req: &DispatchRequest,
        cancel: CancellationToken,
    ) -> Result<NormalizedResponse, ApiError>;
}

/// Keeps every existing harness test green: the stub ignores request inputs exactly as before.
pub struct StubDispatch(pub StubProvider);

#[async_trait]
impl ProviderDispatch for StubDispatch {
    async fn complete(
        &self,
        _req: &DispatchRequest,
        _cancel: CancellationToken,
    ) -> Result<NormalizedResponse, ApiError> {
        self.0.complete().await
    }
}
