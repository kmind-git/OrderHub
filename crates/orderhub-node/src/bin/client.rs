// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software distributed under
//  the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
//  KIND, either express or implied. See the License for the specific language governing
//  permissions and limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Minimal OrderHub client for script acceptance: submits one limit order,
//! prints the admission, snapshots state, and follows the event stream.

use std::time::Duration;

use orderhub_proto::orderhub::v1::journal_event::Kind;
use orderhub_proto::orderhub::v1::order_hub_client::OrderHubClient;
use orderhub_proto::orderhub::v1::{GetStateRequest, SubmitOrderRequest, WatchEventsRequest};
use tonic::Request as RpcRequest;
use tonic::metadata::MetadataValue;

/// Adds the shared bearer token to a request.
fn authorized<T>(
    message: T,
    token: &str,
) -> Result<RpcRequest<T>, tonic::metadata::errors::InvalidMetadataValue> {
    let mut request = RpcRequest::new(message);
    let value: MetadataValue<_> = format!("Bearer {token}").parse()?;
    request.metadata_mut().insert("authorization", value);
    Ok(request)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: std::net::SocketAddr = std::env::var("ORDERHUB_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()?;
    let token =
        std::env::var("ORDERHUB_TOKEN").unwrap_or_else(|_| "orderhub-dev-token".to_string());
    let request_id = std::env::var("ORDERHUB_REQUEST_ID").unwrap_or_else(|_| {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        format!("req-{millis}")
    });

    #[cfg(feature = "grpc-tls")]
    let endpoint = orderhub_gateway::transport::tls_endpoint(
        addr,
        orderhub_gateway::transport::read_pem(
            std::env::var_os("ORDERHUB_TLS_CA").ok_or("grpc-tls requires ORDERHUB_TLS_CA")?,
        )?,
        std::env::var("ORDERHUB_TLS_DOMAIN")
            .map_err(|_| "grpc-tls requires ORDERHUB_TLS_DOMAIN")?,
    )?;
    #[cfg(not(feature = "grpc-tls"))]
    let endpoint = {
        if ["ORDERHUB_TLS_CA", "ORDERHUB_TLS_DOMAIN"]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
        {
            return Err("TLS configuration requires a grpc-tls build".into());
        }
        orderhub_gateway::transport::loopback_endpoint(addr)?
    };
    let channel = endpoint.connect().await?;
    let mut client = OrderHubClient::new(channel);

    let state = client
        .get_state(authorized(GetStateRequest::default(), &token)?)
        .await?
        .into_inner();
    if !state.writable {
        return Err("OrderHub new-order gate is closed".into());
    }
    let stream = client
        .watch_events(authorized(
            WatchEventsRequest {
                after: state.state_cursor.clone(),
                journal_epoch: state.journal_epoch.clone(),
                resume_token: state.resume_token,
            },
            &token,
        )?)
        .await?
        .into_inner();
    let admitted = client
        .submit_order(authorized(
            SubmitOrderRequest {
                journal_epoch: state.journal_epoch,
                submitter_id: String::new(),
                request_id,
                strategy_id: "ORDERHUB-S001".into(),
                instrument_id: "AAPL.GOX".into(),
                side: "BUY".into(),
                quantity: "10".into(),
                price: "153.22".into(),
                time_in_force: "DAY".into(),
                post_only: false,
                reduce_only: false,
                submit_before_ns: None,
            },
            &token,
        )?)
        .await?
        .into_inner();
    println!("REQUEST RESULT: {admitted:?}");
    if admitted.result != "ADMITTED" {
        return Ok(());
    }
    tokio::pin!(stream);
    let mut business_events = 0;
    while business_events < 2 {
        let event = tokio::time::timeout(Duration::from_secs(5), stream.message()).await??;
        let Some(event) = event else {
            return Err("event stream ended; resynchronize state".into());
        };
        match event.kind {
            Some(Kind::Admission(admission)) => {
                println!("REQUEST {}: {}", event.seq, admission.request_id);
                business_events += 1;
            }
            Some(Kind::Outcome(outcome)) => {
                println!("ORDER {}: {}", event.seq, outcome.outcome);
                business_events += 1;
            }
            Some(Kind::Execution(execution)) => {
                println!("EXECUTION {}: {}", event.seq, execution.kind);
                business_events += 1;
            }
            Some(Kind::Progress(_) | Kind::Input(_) | Kind::Control(_)) | None => {}
        }
    }
    println!("Local request and committed events observed");
    Ok(())
}
