//! Dynamic gRPC client: connect to any server, discover its services via
//! server reflection (no compiled .proto needed — this is what grpcurl/
//! Postman itself default to), and make unary calls with a JSON request/
//! response, translated to/from protobuf via prost-reflect's
//! DescriptorPool + DynamicMessage.
//!
//! Deliberately scoped: reflection-based discovery + unary calls only.
//! Raw .proto file import (for servers with reflection disabled) and
//! streaming RPCs aren't implemented — each is a substantial separate
//! feature on top of this.
//!
//! Tonic's built-in Codec machinery expects `Default` to decode a
//! response (`ProstCodec<Req, Resp>`), which `DynamicMessage` can't
//! satisfy meaningfully (a default dynamic message has no descriptor to
//! decode *into*). Rather than fight that, unary calls are framed by
//! hand: gRPC's wire format is just a 5-byte header (1 compression flag
//! byte + 4-byte big-endian length) in front of the protobuf bytes, sent
//! as a single HTTP/2 POST — well-documented and simple enough to do
//! directly against the transport channel.

use anyhow::{anyhow, Context, Result};
use bytes::{Buf, Bytes};
use http_body_util::BodyExt;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MethodDescriptor};
use tonic::transport::{Channel, Endpoint};
use tonic_reflection::pb::v1alpha::server_reflection_client::ServerReflectionClient;
use tonic_reflection::pb::v1alpha::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1alpha::server_reflection_response::MessageResponse;
use tonic_reflection::pb::v1alpha::ServerReflectionRequest;
use tower::ServiceExt;

pub struct GrpcMethodInfo {
    pub service: String,
    pub method: String,
    pub input_type: String,
    pub output_type: String,
    pub client_streaming: bool,
    pub server_streaming: bool,
}

async fn connect(url: &str) -> Result<Channel> {
    Endpoint::from_shared(url.to_string())
        .context("invalid gRPC endpoint URL")?
        .connect()
        .await
        .context("couldn't connect to gRPC server")
}

async fn reflect(channel: Channel, req: MessageRequest) -> Result<MessageResponse> {
    let mut client = ServerReflectionClient::new(channel);
    let request = ServerReflectionRequest {
        host: String::new(),
        message_request: Some(req),
    };
    let stream = tokio_stream::once(request);
    let response = client
        .server_reflection_info(stream)
        .await
        .context("reflection call failed — does this server have reflection enabled?")?;
    let mut inbound = response.into_inner();
    let msg = tonic::codec::Streaming::message(&mut inbound)
        .await?
        .ok_or_else(|| anyhow!("empty reflection response"))?;
    msg.message_response
        .ok_or_else(|| anyhow!("reflection response had no payload"))
}

/// Every service name the server's reflection endpoint advertises
/// (excluding the reflection service itself).
pub async fn list_services(url: &str) -> Result<Vec<String>> {
    let channel = connect(url).await?;
    let resp = reflect(channel, MessageRequest::ListServices(String::new())).await?;
    match resp {
        MessageResponse::ListServicesResponse(l) => Ok(l
            .service
            .into_iter()
            .map(|s| s.name)
            .filter(|n| !n.starts_with("grpc.reflection."))
            .collect()),
        MessageResponse::ErrorResponse(e) => Err(anyhow!("reflection error: {}", e.error_message)),
        _ => Err(anyhow!("unexpected reflection response to ListServices")),
    }
}

async fn descriptor_pool_for(channel: Channel, symbol: &str) -> Result<DescriptorPool> {
    let resp = reflect(channel, MessageRequest::FileContainingSymbol(symbol.to_string())).await?;
    let fdr = match resp {
        MessageResponse::FileDescriptorResponse(fdr) => fdr,
        MessageResponse::ErrorResponse(e) => return Err(anyhow!("reflection error: {}", e.error_message)),
        _ => return Err(anyhow!("unexpected reflection response to FileContainingSymbol")),
    };
    // The server sends every file transitively needed, but not
    // necessarily in dependency order — add_file_descriptor_proto
    // requires each file's own dependencies to already be in the pool,
    // so retry in passes until nothing new can be added (or nothing's
    // left, or we're stuck, which means a truly missing dependency).
    let mut pending: Vec<prost_types::FileDescriptorProto> = fdr
        .file_descriptor_proto
        .iter()
        .map(|b| prost_types::FileDescriptorProto::decode(b.as_slice()))
        .collect::<std::result::Result<_, _>>()
        .context("server sent an invalid FileDescriptorProto")?;
    let mut pool = DescriptorPool::new();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|fdp| pool.add_file_descriptor_proto(fdp.clone()).is_err());
        if pending.len() == before {
            return Err(anyhow!(
                "couldn't resolve proto dependencies for {symbol} (missing: {:?})",
                pending.iter().map(|f| f.name()).collect::<Vec<_>>()
            ));
        }
    }
    Ok(pool)
}

/// Every method on `service`, with its request/response type names —
/// enough for the UI to show a form and for `call_unary` to look the
/// descriptors back up by name.
pub async fn list_methods(url: &str, service: &str) -> Result<Vec<GrpcMethodInfo>> {
    let channel = connect(url).await?;
    let pool = descriptor_pool_for(channel, service).await?;
    let svc = pool
        .get_service_by_name(service)
        .ok_or_else(|| anyhow!("service '{service}' not found in its own descriptor"))?;
    Ok(svc
        .methods()
        .map(|m| GrpcMethodInfo {
            service: service.to_string(),
            method: m.name().to_string(),
            input_type: m.input().full_name().to_string(),
            output_type: m.output().full_name().to_string(),
            client_streaming: m.is_client_streaming(),
            server_streaming: m.is_server_streaming(),
        })
        .collect())
}

fn encode_grpc_frame(msg: &DynamicMessage) -> Result<Bytes> {
    let mut payload = Vec::new();
    msg.encode(&mut payload).context("failed to encode request message")?;
    let mut framed = Vec::with_capacity(payload.len() + 5);
    framed.push(0u8); // not compressed
    framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    framed.extend_from_slice(&payload);
    Ok(Bytes::from(framed))
}

async fn decode_grpc_frame(body: Bytes, desc: &prost_reflect::MessageDescriptor) -> Result<DynamicMessage> {
    let mut buf = body;
    if buf.remaining() < 5 {
        return Err(anyhow!("response body too short to be a gRPC frame"));
    }
    let _compressed = buf.get_u8();
    let len = buf.get_u32() as usize;
    if buf.remaining() < len {
        return Err(anyhow!("truncated gRPC frame"));
    }
    let payload = buf.copy_to_bytes(len);
    DynamicMessage::decode(desc.clone(), payload).context("failed to decode response message")
}

/// Finds `service`'s `method` and returns its input/output descriptors,
/// for building a request form or validating one before calling.
pub async fn resolve_method(url: &str, service: &str, method: &str) -> Result<MethodDescriptor> {
    let channel = connect(url).await?;
    let pool = descriptor_pool_for(channel, service).await?;
    let svc = pool
        .get_service_by_name(service)
        .ok_or_else(|| anyhow!("service '{service}' not found"))?;
    let found = svc
        .methods()
        .find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("method '{method}' not found on service '{service}'"))?;
    Ok(found)
}

/// Makes one unary gRPC call. `json_payload` is deserialized against the
/// method's actual input message descriptor (so field names/types are
/// exactly what the .proto defines, no separate schema to keep in sync)
/// and the response comes back as pretty-printed JSON the same way.
pub async fn call_unary(url: &str, service: &str, method: &str, json_payload: &str) -> Result<String> {
    let channel = connect(url).await?;
    let pool = descriptor_pool_for(channel.clone(), service).await?;
    let svc = pool
        .get_service_by_name(service)
        .ok_or_else(|| anyhow!("service '{service}' not found"))?;
    let method_desc = svc
        .methods()
        .find(|m| m.name() == method)
        .ok_or_else(|| anyhow!("method '{method}' not found on service '{service}'"))?;
    if method_desc.is_client_streaming() || method_desc.is_server_streaming() {
        return Err(anyhow!("'{method}' is a streaming RPC — only unary calls are supported"));
    }

    let json_value: serde_json::Value =
        serde_json::from_str(json_payload).context("request payload isn't valid JSON")?;
    let request_msg = DynamicMessage::deserialize(method_desc.input(), json_value)
        .context("request JSON doesn't match the method's input message shape")?;
    let frame = encode_grpc_frame(&request_msg)?;

    let path = format!("/{}/{}", service, method);
    let req = http::Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(tonic::body::Body::new(http_body_util::Full::new(frame)))
        .context("failed to build gRPC request")?;

    let response = channel
        .oneshot(req)
        .await
        .map_err(|e| anyhow!("gRPC transport error: {e}"))?;
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .context("failed to read gRPC response body")?
        .to_bytes();
    if !status.is_success() {
        return Err(anyhow!("gRPC call failed with HTTP status {status}"));
    }

    let response_msg = decode_grpc_frame(body, &method_desc.output()).await?;
    serde_json::to_string_pretty(&response_msg).context("failed to serialize response as JSON")
}
