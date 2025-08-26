use crate::config::Config;
use crate::connection_tracker::{generate_connection_id, EventSender, SharedConnectionTracker};
use crate::events::{ConnectionStatus, ProxyEvent, SelectionMode};
use crate::health::EndpointStatus;
use crate::logging::*;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Client, Request, Response, Server, StatusCode, Uri};
use hyper_tls::HttpsConnector;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tracing::error;

pub type SharedState = Arc<Mutex<ProxyState>>;

#[derive(Debug)]
pub struct ProxyState {
    pub config: Config,
    pub endpoint_status: HashMap<String, EndpointStatus>,
    pub current_endpoint: String,
    pub selection_mode: SelectionMode,
}

impl ProxyState {
    pub fn new(config: Config) -> Self {
        // Get default endpoint from new config structure
        let current_endpoint = if let Some((_, default_endpoint)) = config.get_default_endpoint() {
            default_endpoint.url.clone()
        } else {
            // Fallback to first available endpoint
            config
                .get_all_endpoints()
                .first()
                .map(|(_, endpoint, _)| endpoint.url.clone())
                .unwrap_or_default()
        };

        let mut endpoint_status = HashMap::new();

        // Initialize all endpoints as unavailable
        for (_, endpoint, _) in config.get_all_endpoints() {
            endpoint_status.insert(
                endpoint.url.clone(),
                EndpointStatus::new_unavailable(
                    endpoint.url.clone(),
                    "Not checked yet".to_string(),
                ),
            );
        }

        Self {
            config,
            endpoint_status,
            current_endpoint,
            selection_mode: SelectionMode::Auto, // Start with auto mode
        }
    }

    pub fn switch_endpoint(&mut self, new_endpoint: String) {
        if new_endpoint != self.current_endpoint {
            let from_latency = self
                .endpoint_status
                .get(&self.current_endpoint)
                .map(|s| s.latency)
                .unwrap_or(999999);
            let to_latency = self
                .endpoint_status
                .get(&new_endpoint)
                .map(|s| s.latency)
                .unwrap_or(999999);

            log_endpoint_switch(
                &self.current_endpoint,
                &new_endpoint,
                from_latency,
                to_latency,
            );
            self.current_endpoint = new_endpoint;
        }
    }

    pub fn switch_endpoint_silent(&mut self, new_endpoint: String) {
        if new_endpoint != self.current_endpoint {
            // No console log for dashboard mode
            self.current_endpoint = new_endpoint;
        }
    }
}

#[allow(dead_code)]
pub async fn start_proxy_server(config: Config, state: SharedState) -> anyhow::Result<()> {
    let https = HttpsConnector::new();
    let client = Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(4)
        .build::<_, hyper::Body>(https);

    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();
        let client = client.clone();

        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let state = state.clone();
                let client = client.clone();

                async move {
                    match handle_request(req, state, client).await {
                        Ok(response) => Ok::<Response<Body>, hyper::Error>(response),
                        Err(e) => {
                            error!("Request error: {}", e);
                            Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error"))
                                .unwrap())
                        }
                    }
                }
            }))
        }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], config.server.port));
    let server = Server::bind(&addr).serve(make_svc);

    log_server_start(config.server.port);

    if let Err(e) = server.await {
        log_server_error(&format!("{e}"));
        return Err(anyhow::anyhow!("Server error: {}", e));
    }

    Ok(())
}

/// Start proxy server with event and connection tracking support for dashboard mode (no console logs)
pub async fn start_proxy_server_with_events_dashboard(
    config: Config,
    state: SharedState,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
) -> anyhow::Result<()> {
    let https = HttpsConnector::new();
    let client = Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(4)
        .build::<_, hyper::Body>(https);

    // Send server started event before creating the service (no console log)
    let _ = event_sender.send(ProxyEvent::ServerStarted {
        port: config.server.port,
    });

    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();
        let client = client.clone();
        let tracker = connection_tracker.clone();
        let sender = event_sender.clone();

        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let state = state.clone();
                let client = client.clone();
                let tracker = tracker.clone();
                let sender = sender.clone();

                async move {
                    match handle_request_with_events_dashboard(req, state, client, tracker, sender)
                        .await
                    {
                        Ok(response) => Ok::<Response<Body>, hyper::Error>(response),
                        Err(e) => {
                            error!("Request error: {}", e);
                            Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error"))
                                .unwrap())
                        }
                    }
                }
            }))
        }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], config.server.port));
    let server = Server::bind(&addr).serve(make_svc);

    // No console log for dashboard mode

    if let Err(e) = server.await {
        return Err(anyhow::anyhow!("Server error: {}", e));
    }

    Ok(())
}

pub async fn start_proxy_server_with_events(
    config: Config,
    state: SharedState,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
) -> anyhow::Result<()> {
    let https = HttpsConnector::new();
    let client = Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(4)
        .build::<_, hyper::Body>(https);

    // Send server started event before creating the service
    let _ = event_sender.send(ProxyEvent::ServerStarted {
        port: config.server.port,
    });

    let make_svc = make_service_fn(move |_conn| {
        let state = state.clone();
        let client = client.clone();
        let tracker = connection_tracker.clone();
        let sender = event_sender.clone();

        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let state = state.clone();
                let client = client.clone();
                let tracker = tracker.clone();
                let sender = sender.clone();

                async move {
                    match handle_request_with_events(req, state, client, tracker, sender).await {
                        Ok(response) => Ok::<Response<Body>, hyper::Error>(response),
                        Err(e) => {
                            error!("Request error: {}", e);
                            Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error"))
                                .unwrap())
                        }
                    }
                }
            }))
        }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], config.server.port));
    let server = Server::bind(&addr).serve(make_svc);

    log_server_start(config.server.port);

    if let Err(e) = server.await {
        log_server_error(&format!("{e}"));
        return Err(anyhow::anyhow!("Server error: {}", e));
    }

    Ok(())
}

async fn handle_request_with_events(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
) -> anyhow::Result<Response<Body>> {
    match req.uri().path() {
        "/status" => status_handler(state, Some(connection_tracker.clone())).await,
        "/diagnostics" => diagnostics_handler(connection_tracker.clone()).await,
        "/health" => health_handler().await,
        _ => proxy_handler_with_events(req, state, client, connection_tracker, event_sender).await,
    }
}

async fn handle_request_with_events_dashboard(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
) -> anyhow::Result<Response<Body>> {
    match req.uri().path() {
        "/status" => status_handler(state, Some(connection_tracker.clone())).await,
        "/diagnostics" => diagnostics_handler(connection_tracker.clone()).await,
        "/health" => health_handler().await,
        _ => {
            proxy_handler_with_events_dashboard(
                req,
                state,
                client,
                connection_tracker,
                event_sender,
            )
            .await
        }
    }
}

#[allow(dead_code)]
async fn handle_request(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
) -> anyhow::Result<Response<Body>> {
    match req.uri().path() {
        "/status" => status_handler(state, None).await,
        "/health" => health_handler().await,
        _ => proxy_handler(req, state, client).await,
    }
}

async fn proxy_handler_with_events_impl(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
    silent_mode: bool, // true for dashboard mode (no logs), false for normal mode
) -> anyhow::Result<Response<Body>> {
    // Generate unique connection ID
    let connection_id = generate_connection_id();

    // Get the current endpoint and corresponding auth token for this request
    let (endpoint_for_request, auth_token) = {
        let state_guard = state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire state lock: {}", e))?;
        let current_endpoint = state_guard.current_endpoint.clone();

        // Find the auth token for this endpoint
        let auth_token = state_guard
            .config
            .get_all_endpoints()
            .into_iter()
            .find(|(_, endpoint, _)| endpoint.url == current_endpoint)
            .map(|(token, _, _)| token)
            .unwrap_or_default();

        (current_endpoint, auth_token)
    };

    // Build the target URI
    let uri_string = format!(
        "{}{}",
        endpoint_for_request,
        req.uri().path_and_query().map(|x| x.as_str()).unwrap_or("")
    );
    let uri: Uri = uri_string
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid URI: {}", e))?;

    // Create new request to target
    let (mut parts, body) = req.into_parts();
    parts.uri = uri;

    // Extract host from the endpoint URL
    let host = endpoint_for_request
        .strip_prefix("https://")
        .or_else(|| endpoint_for_request.strip_prefix("http://"))
        .unwrap_or(&endpoint_for_request);
    if let Ok(host_value) = host.parse() {
        parts.headers.insert("host", host_value);
    }

    // Remove any existing Authorization header from the original request
    parts.headers.remove("authorization");

    // Add Authorization header with the auth token from config
    if !auth_token.is_empty() {
        let auth_value = format!("Bearer {auth_token}");
        if let Ok(auth_header) = auth_value.parse() {
            parts.headers.insert("authorization", auth_header);
        }
    }

    let new_req = Request::from_parts(parts, body);

    // Start connection tracking and set to processing in single lock acquisition
    let active_connection = {
        let mut tracker = connection_tracker.lock().unwrap();
        let connection = tracker.start_connection(connection_id.clone(), endpoint_for_request.clone());
        tracker.update_connection_status(&connection_id, ConnectionStatus::Processing);
        connection
    };

    // Send connection started event
    let _ = event_sender.send(ProxyEvent::ConnectionStarted(active_connection));

    // Send request received event for load tracking
    let _ = event_sender.send(ProxyEvent::RequestReceived {
        endpoint: endpoint_for_request.clone(),
        timestamp: std::time::Instant::now(),
    });

    // Log proxy request only if not in silent mode
    if !silent_mode {
        log_proxy_request(&endpoint_for_request);
    }

    // Forward request with timeout - This will block for the entire duration of the AI response
    // For AI responses, this await can take 30+ seconds for long content generation
    // Set a generous timeout for AI responses (5 minutes)
    let timeout_duration = std::time::Duration::from_secs(300); // 5 minutes
    let response = tokio::time::timeout(timeout_duration, client.request(new_req)).await;

    match response {
        Ok(Ok(mut resp)) => {
            // Response headers received, but AI might still be generating content
            // Keep status as Processing during body transmission

            // For streaming responses, we need to consume the entire body to ensure
            // the connection represents the true end-to-end time
            // Apply timeout to body consumption as well to prevent hanging on stalled streams
            let body_bytes = tokio::time::timeout(
                std::time::Duration::from_secs(300), // Same 5-minute timeout
                hyper::body::to_bytes(resp.body_mut()),
            )
            .await??;

            // NOW the AI has finished generating and transmitting - update to finishing
            {
                let mut tracker = connection_tracker.lock().unwrap();
                tracker.update_connection_status(&connection_id, ConnectionStatus::Finishing);
            }

            let new_body = Body::from(body_bytes);

            // Create new response with the consumed body
            let (parts, _) = resp.into_parts();
            let final_response = Response::from_parts(parts, new_body);

            // Complete connection tracking - connection is truly finished
            {
                let mut tracker = connection_tracker.lock().unwrap();
                tracker.complete_connection(&connection_id);
            }

            // Send connection completed event
            let _ = event_sender.send(ProxyEvent::ConnectionCompleted(connection_id.clone()));

            Ok(final_response)
        }
        Ok(Err(e)) => {
            // HTTP request error
            // Log proxy error only if not in silent mode
            if !silent_mode {
                log_proxy_error(&endpoint_for_request, &format!("HTTP error: {e}"));
            }

            // Mark the endpoint we actually used as failed
            {
                let mut state_guard = state.lock().unwrap();
                if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request) {
                    status.available = false;
                    status.error = Some(format!("HTTP error: {e}"));
                    status.last_check = chrono::Utc::now();
                }
            }

            // Complete connection tracking even on error
            {
                let mut tracker = connection_tracker.lock().unwrap();
                tracker.complete_connection(&connection_id);
            }

            // Send connection completed event
            let _ = event_sender.send(ProxyEvent::ConnectionCompleted(connection_id.clone()));

            Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("HTTP error"))?)
        }
        Err(_timeout) => {
            // Timeout error
            // Log proxy error only if not in silent mode
            if !silent_mode {
                log_proxy_error(&endpoint_for_request, "Request timeout (5 minutes)");
            }

            // Mark the endpoint we actually used as failed
            {
                let mut state_guard = state.lock().unwrap();
                if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request) {
                    status.available = false;
                    status.error = Some("Request timeout".to_string());
                    status.last_check = chrono::Utc::now();
                }
            }

            // Complete connection tracking on timeout
            {
                let mut tracker = connection_tracker.lock().unwrap();
                tracker.complete_connection(&connection_id);
            }

            // Send connection completed event
            let _ = event_sender.send(ProxyEvent::ConnectionCompleted(connection_id.clone()));

            Ok(Response::builder()
                .status(StatusCode::GATEWAY_TIMEOUT)
                .body(Body::from("Request timeout"))?)
        }
    }
}

async fn proxy_handler_with_events(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
) -> anyhow::Result<Response<Body>> {
    proxy_handler_with_events_impl(req, state, client, connection_tracker, event_sender, false)
        .await
}

async fn proxy_handler_with_events_dashboard(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    connection_tracker: SharedConnectionTracker,
    event_sender: EventSender,
) -> anyhow::Result<Response<Body>> {
    proxy_handler_with_events_impl(req, state, client, connection_tracker, event_sender, true).await
}

#[allow(dead_code)]
async fn proxy_handler(
    req: Request<Body>,
    state: SharedState,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
) -> anyhow::Result<Response<Body>> {
    // Get the current endpoint and corresponding auth token for this request
    let (endpoint_for_request, auth_token) = {
        let state_guard = state
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire state lock: {}", e))?;
        let current_endpoint = state_guard.current_endpoint.clone();

        // Find the auth token for this endpoint
        let auth_token = state_guard
            .config
            .get_all_endpoints()
            .into_iter()
            .find(|(_, endpoint, _)| endpoint.url == current_endpoint)
            .map(|(token, _, _)| token)
            .unwrap_or_default();

        (current_endpoint, auth_token)
    };

    // Build the target URI
    let uri_string = format!(
        "{}{}",
        endpoint_for_request,
        req.uri().path_and_query().map(|x| x.as_str()).unwrap_or("")
    );
    let uri: Uri = uri_string
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid URI: {}", e))?;

    // Create new request to target
    let (mut parts, body) = req.into_parts();
    parts.uri = uri;

    // Extract host from the endpoint URL
    let host = endpoint_for_request
        .strip_prefix("https://")
        .or_else(|| endpoint_for_request.strip_prefix("http://"))
        .unwrap_or(&endpoint_for_request);
    if let Ok(host_value) = host.parse() {
        parts.headers.insert("host", host_value);
    }

    // Remove any existing Authorization header from the original request
    parts.headers.remove("authorization");

    // Add Authorization header with the auth token from config
    if !auth_token.is_empty() {
        let auth_value = format!("Bearer {auth_token}");
        if let Ok(auth_header) = auth_value.parse() {
            parts.headers.insert("authorization", auth_header);
        }
    }

    let new_req = Request::from_parts(parts, body);

    log_proxy_request(&endpoint_for_request);

    // Forward request with timeout
    let timeout_duration = std::time::Duration::from_secs(300); // 5 minutes
    let response = tokio::time::timeout(timeout_duration, client.request(new_req)).await;

    match response {
        Ok(Ok(resp)) => Ok(resp),
        Ok(Err(e)) => {
            log_proxy_error(&endpoint_for_request, &format!("HTTP error: {e}"));

            // Mark the endpoint we actually used as failed
            {
                let mut state_guard = state.lock().unwrap();
                if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request) {
                    status.available = false;
                    status.error = Some(format!("HTTP error: {e}"));
                    status.last_check = chrono::Utc::now();
                }
            }

            Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("HTTP error"))?)
        }
        Err(_timeout) => {
            log_proxy_error(&endpoint_for_request, "Request timeout (5 minutes)");

            // Mark the endpoint we actually used as failed
            {
                let mut state_guard = state.lock().unwrap();
                if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request) {
                    status.available = false;
                    status.error = Some("Request timeout".to_string());
                    status.last_check = chrono::Utc::now();
                }
            }

            Ok(Response::builder()
                .status(StatusCode::GATEWAY_TIMEOUT)
                .body(Body::from("Request timeout"))?)
        }
    }
}

async fn diagnostics_handler(
    connection_tracker: SharedConnectionTracker,
) -> anyhow::Result<Response<Body>> {
    let diagnostics = {
        let tracker = connection_tracker.lock().unwrap();
        tracker.get_connection_diagnostics()
    };

    let response_json = serde_json::json!({
        "connection_diagnostics": {
            "total_active": diagnostics.total_active,
            "endpoint_distribution": diagnostics.endpoint_counts,
            "connection_durations": diagnostics.duration_stats,
            "completed_count": diagnostics.completed_count,
            "peak_concurrent": diagnostics.peak_concurrent,
            "longest_connection_seconds": diagnostics.duration_stats.iter().max().unwrap_or(&0),
            "average_duration_seconds": if diagnostics.duration_stats.is_empty() { 
                0 
            } else { 
                diagnostics.duration_stats.iter().sum::<u64>() / diagnostics.duration_stats.len() as u64 
            }
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(response_json.to_string()))?)
}

async fn status_handler(
    state: SharedState,
    connection_tracker: Option<SharedConnectionTracker>,
) -> anyhow::Result<Response<Body>> {
    let state_guard = state.lock().unwrap();

    // Get connection info from tracker if available, otherwise use old system for backwards compatibility
    let (total_active_connections, endpoint_distribution) =
        if let Some(ref tracker) = connection_tracker {
            if let Ok(tracker_guard) = tracker.lock() {
                (
                    tracker_guard.get_active_count(),
                    tracker_guard.get_endpoint_distribution().clone(),
                )
            } else {
                (0, std::collections::HashMap::new())
            }
        } else {
            (0, std::collections::HashMap::new())
        };

    let status_info = serde_json::json!({
        "current_endpoint": state_guard.current_endpoint,
        "total_active_connections": total_active_connections,
        "endpoint_connections": endpoint_distribution,
        "endpoints": state_guard.endpoint_status,
        "timestamp": chrono::Utc::now(),
        "config": {
            "port": state_guard.config.server.port,
            "switch_threshold_ms": state_guard.config.server.switch_threshold_ms,
            "health_check_interval_seconds": state_guard.config.health_check.interval_seconds,
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string_pretty(&status_info)?))?)
}

async fn health_handler() -> anyhow::Result<Response<Body>> {
    Ok(Response::builder()
        .status(StatusCode::OK)
        .body(Body::from("OK"))?)
}
