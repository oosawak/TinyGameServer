use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub service: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsureGroupResponse {
    pub group_id: String,
    pub port: u16,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValuePayload {
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Delivery {
    Reliable,
    Unreliable,
}
