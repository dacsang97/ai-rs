use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalResponse {
    Approved,
    Denied { message: Option<String> },
}

pub struct ApprovalRequest {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub response_tx: tokio::sync::oneshot::Sender<ApprovalResponse>,
}
