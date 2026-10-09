//! The messages of `api/proto/flaminio/meta/v1/meta.proto` that the data plane uses, written
//! with prost's derives instead of generated, so the build needs no `protoc`. Field numbers
//! must match the proto file; the interop test against the Go server checks them.

#[derive(Clone, PartialEq, prost::Message)]
pub struct Node {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(string, tag = "2")]
    pub address: String,
    #[prost(map = "string, string", tag = "3")]
    pub labels: std::collections::HashMap<String, String>,
    #[prost(bool, tag = "4")]
    pub alive: bool,
    #[prost(bool, tag = "5")]
    pub draining: bool,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct RegisterNodeRequest {
    #[prost(message, optional, tag = "1")]
    pub node: Option<Node>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct RegisterNodeResponse {}

#[derive(Clone, PartialEq, prost::Message)]
pub struct NodeHeartbeatRequest {
    #[prost(string, tag = "1")]
    pub node: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct NodeHeartbeatResponse {
    #[prost(uint32, tag = "1")]
    pub ttl_seconds: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ChunkMeta {
    #[prost(string, repeated, tag = "1")]
    pub replicas: Vec<String>,
    #[prost(string, repeated, tag = "2")]
    pub isr: Vec<String>,
    #[prost(uint64, tag = "3")]
    pub isr_ver: u64,
    /// A requested move: empty if none.
    #[prost(string, tag = "4")]
    pub moving_from: String,
    #[prost(string, tag = "5")]
    pub moving_to: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Volume {
    #[prost(string, tag = "1")]
    pub id: String,
    #[prost(uint64, tag = "2")]
    pub size_bytes: u64,
    #[prost(uint64, tag = "3")]
    pub chunk_bytes: u64,
    #[prost(uint32, tag = "4")]
    pub replicas: u32,
    #[prost(uint32, tag = "5")]
    pub min_isr: u32,
    #[prost(uint64, tag = "6")]
    pub epoch: u64,
    #[prost(string, tag = "7")]
    pub target: String,
    #[prost(message, repeated, tag = "8")]
    pub chunks: Vec<ChunkMeta>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct GetVolumeRequest {
    #[prost(string, tag = "1")]
    pub id: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct GetVolumeResponse {
    #[prost(message, optional, tag = "1")]
    pub volume: Option<Volume>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct IsrChange {
    #[prost(uint32, tag = "1")]
    pub chunk: u32,
    #[prost(uint64, tag = "2")]
    pub expected_ver: u64,
    #[prost(string, repeated, tag = "3")]
    pub new_isr: Vec<String>,
    #[prost(bool, tag = "4")]
    pub change_replicas: bool,
    #[prost(string, repeated, tag = "5")]
    pub new_replicas: Vec<String>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ChangeIsrRequest {
    #[prost(string, tag = "1")]
    pub volume: String,
    #[prost(uint64, tag = "2")]
    pub epoch: u64,
    #[prost(message, repeated, tag = "3")]
    pub changes: Vec<IsrChange>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ChangeIsrResponse {
    #[prost(bool, tag = "1")]
    pub applied: bool,
    #[prost(uint64, tag = "2")]
    pub epoch: u64,
    #[prost(message, repeated, tag = "3")]
    pub chunks: Vec<ChunkMeta>,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ListNodesRequest {}

#[derive(Clone, PartialEq, prost::Message)]
pub struct ListNodesResponse {
    #[prost(message, repeated, tag = "1")]
    pub nodes: Vec<Node>,
}
