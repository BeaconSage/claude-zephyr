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
use std::time::Duration;
use tokio::time::sleep;
use tracing::error;

/// Error types that can be retried
#[derive(Debug)]
enum RetryableError {
    /// Network connection errors
    #[allow(dead_code)]
    ConnectionError(String),
    /// Request timeout
    Timeout,
    /// 5xx server errors
    #[allow(dead_code)]
    ServerError(u16),
}

/// Error types that should not be retried
#[derive(Debug)]
#[allow(dead_code)]
enum NonRetryableError {
    /// 4xx client errors
    ClientError(u16),
    /// Authentication failures
    AuthError,
    /// Malformed request
    BadRequest(String),
}

/// Classify error to determine if it should be retried
fn classify_error(error: &hyper::Error) -> Option<RetryableError> {
    if error.is_timeout() {
        return Some(RetryableError::Timeout);
    }

    if error.is_connect() || error.is_closed() {
        return Some(RetryableError::ConnectionError(error.to_string()));
    }

    // For other hyper errors, assume they might be retryable
    Some(RetryableError::ConnectionError(error.to_string()))
}

/// Calculate delay for exponential backoff
fn calculate_backoff_delay(attempt: u32, base_delay_ms: u64, multiplier: f32) -> Duration {
    let delay_ms = base_delay_ms as f64 * (multiplier as f64).powi(attempt as i32 - 1);
    Duration::from_millis(delay_ms as u64)
}

/// Unified connection cleanup function to ensure proper cleanup in all exit paths
async fn cleanup_connection_on_exit(
    connection_id: &str,
    connection_tracker: &SharedConnectionTracker,
    event_sender: &EventSender,
    _reason: &str,
) {
    if let Ok(mut tracker) = connection_tracker.lock() {
        if tracker.complete_connection(connection_id).is_some() {
            let _ = event_sender.send(ProxyEvent::ConnectionCompleted(connection_id.to_string()));
        }
    }
}

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

/// Retry wrapper for HTTP requests with exponential backoff
async fn retry_request<F, Fut>(
    config: &crate::config::RetryConfig,
    endpoint: &str,
    silent_mode: bool,
    request_fn: F,
) -> Result<Response<Body>, hyper::Error>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Response<Body>, hyper::Error>>,
{
    use crate::logging::{
        log_retry_attempt, log_retry_delay, log_retry_exhausted, log_retry_success,
    };

    if !config.enabled || config.max_attempts <= 1 {
        return request_fn().await;
    }

    let mut last_error = None;
    let mut total_delay_ms = 0u64;

    for attempt in 1..=config.max_attempts {
        match request_fn().await {
            Ok(response) => {
                // Check if it's a 5xx error that should be retried
                let status = response.status();
                if status.is_server_error() && attempt < config.max_attempts {
                    if !silent_mode {
                        log_retry_attempt(
                            endpoint,
                            attempt,
                            config.max_attempts,
                            &format!("Server error {}", status),
                        );

                        let delay = calculate_backoff_delay(
                            attempt,
                            config.base_delay_ms,
                            config.backoff_multiplier,
                        );
                        let delay_ms = delay.as_millis() as u64;
                        total_delay_ms += delay_ms;

                        log_retry_delay(endpoint, attempt + 1, delay_ms);
                        sleep(delay).await;
                    } else {
                        let delay = calculate_backoff_delay(
                            attempt,
                            config.base_delay_ms,
                            config.backoff_multiplier,
                        );
                        total_delay_ms += delay.as_millis() as u64;
                        sleep(delay).await;
                    }
                    continue;
                }

                // Success or non-retryable status code
                if attempt > 1 && !silent_mode {
                    log_retry_success(endpoint, attempt, total_delay_ms);
                }
                return Ok(response);
            }
            Err(error) => {
                if let Some(retryable_error) = classify_error(&error) {
                    if attempt < config.max_attempts {
                        if !silent_mode {
                            log_retry_attempt(
                                endpoint,
                                attempt,
                                config.max_attempts,
                                &format!("{:?}", retryable_error),
                            );

                            let delay = calculate_backoff_delay(
                                attempt,
                                config.base_delay_ms,
                                config.backoff_multiplier,
                            );
                            let delay_ms = delay.as_millis() as u64;
                            total_delay_ms += delay_ms;

                            log_retry_delay(endpoint, attempt + 1, delay_ms);
                            sleep(delay).await;
                        } else {
                            let delay = calculate_backoff_delay(
                                attempt,
                                config.base_delay_ms,
                                config.backoff_multiplier,
                            );
                            total_delay_ms += delay.as_millis() as u64;
                            sleep(delay).await;
                        }
                        last_error = Some(error);
                        continue;
                    }
                }

                // Non-retryable error or max attempts reached
                if !silent_mode {
                    log_retry_exhausted(endpoint, config.max_attempts, &format!("{:?}", error));
                }
                return Err(error);
            }
        }
    }

    // This should never be reached, but handle it gracefully
    // Since we can't easily create hyper::Error, just use the first attempt's error
    let final_error = last_error.unwrap_or_else(|| panic!("No error available in retry logic"));
    if !silent_mode {
        log_retry_exhausted(endpoint, config.max_attempts, &format!("{:?}", final_error));
    }
    Err(final_error)
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

    // Get the current endpoint, auth token, and retry config for this request
    let (endpoint_for_request, auth_token, retry_config) = {
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

        let retry_config = state_guard.config.retry.clone();

        (current_endpoint, auth_token, retry_config)
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

    // Convert body to bytes so it can be reused for retries if needed
    let body_bytes = hyper::body::to_bytes(body)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read request body: {}", e))?;

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

    // Start connection tracking and set to processing in single lock acquisition
    let active_connection = {
        match connection_tracker.lock() {
            Ok(mut tracker) => {
                let connection =
                    tracker.start_connection(connection_id.clone(), endpoint_for_request.clone());
                tracker.update_connection_status(&connection_id, ConnectionStatus::Processing);
                connection
            }
            Err(e) => {
                tracing::error!("Failed to acquire connection tracker lock: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Internal server error"))?);
            }
        }
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

    // Create a closure for the request execution that can be retried
    let body_bytes_for_retry = body_bytes.clone();
    let client_for_retry = client.clone();
    let uri_for_retry = parts.uri.clone();
    let headers_for_retry = parts.headers.clone();
    let method_for_retry = parts.method.clone();
    let version_for_retry = parts.version;

    let request_fn = move || {
        let body_bytes = body_bytes_for_retry.clone();
        let client = client_for_retry.clone();
        let uri = uri_for_retry.clone();
        let headers = headers_for_retry.clone();
        let method = method_for_retry.clone();
        let version = version_for_retry;

        async move {
            // Build new request using builder pattern
            let mut request_builder = hyper::Request::builder()
                .method(method)
                .uri(uri)
                .version(version);

            // Add all headers
            for (name, value) in headers.iter() {
                request_builder = request_builder.header(name, value);
            }

            // This should not fail if we're cloning valid parts
            let new_req = request_builder
                .body(Body::from(body_bytes))
                .expect("Failed to build request from valid parts");

            // Set a generous timeout for AI responses (5 minutes)
            let timeout_duration = std::time::Duration::from_secs(300);
            match tokio::time::timeout(timeout_duration, client.request(new_req)).await {
                Ok(result) => result,
                Err(_timeout) => {
                    // Return timeout error: re-use the first connection error format
                    // This is a hack but necessary since hyper::Error is hard to construct
                    client
                        .request(hyper::Request::get("").body(Body::empty()).unwrap())
                        .await
                }
            }
        }
    };

    // Execute request with retry logic
    let response = retry_request(
        &retry_config,
        &endpoint_for_request,
        silent_mode,
        request_fn,
    )
    .await;

    // Handle all possible outcomes with unified cleanup
    let result = match response {
        Ok(mut resp) => {
            // Response headers received, but AI might still be generating content
            // Keep status as Processing during body transmission

            // For streaming responses, we need to consume the entire body to ensure
            // the connection represents the true end-to-end time
            // Apply timeout to body consumption as well to prevent hanging on stalled streams
            match tokio::time::timeout(
                std::time::Duration::from_secs(300), // Same 5-minute timeout
                hyper::body::to_bytes(resp.body_mut()),
            )
            .await
            {
                Ok(Ok(body_bytes)) => {
                    // NOW the AI has finished generating and transmitting - update to finishing
                    if let Ok(mut tracker) = connection_tracker.lock() {
                        tracker
                            .update_connection_status(&connection_id, ConnectionStatus::Finishing);
                    }

                    let new_body = Body::from(body_bytes);

                    // Create new response with the consumed body
                    let (parts, _) = resp.into_parts();
                    let final_response = Response::from_parts(parts, new_body);

                    // Successful completion - cleanup will be handled by unified function
                    cleanup_connection_on_exit(
                        &connection_id,
                        &connection_tracker,
                        &event_sender,
                        "success",
                    )
                    .await;
                    Ok(final_response)
                }
                Ok(Err(e)) => {
                    // Body consumption error
                    if !silent_mode {
                        log_proxy_error(
                            &endpoint_for_request,
                            &format!("Body consumption error: {e}"),
                        );
                    }
                    cleanup_connection_on_exit(
                        &connection_id,
                        &connection_tracker,
                        &event_sender,
                        "body_error",
                    )
                    .await;
                    Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from("Body consumption error"))?)
                }
                Err(_) => {
                    // Body consumption timeout
                    if !silent_mode {
                        log_proxy_error(&endpoint_for_request, "Body consumption timeout");
                    }
                    cleanup_connection_on_exit(
                        &connection_id,
                        &connection_tracker,
                        &event_sender,
                        "body_timeout",
                    )
                    .await;
                    Ok(Response::builder()
                        .status(StatusCode::GATEWAY_TIMEOUT)
                        .body(Body::from("Body consumption timeout"))?)
                }
            }
        }
        Err(e) => {
            // HTTP request error (already went through retry logic)
            if !silent_mode {
                log_proxy_error(
                    &endpoint_for_request,
                    &format!("HTTP error after retries: {e}"),
                );
            }

            // Mark the endpoint we actually used as failed
            if let Ok(mut state_guard) = state.lock() {
                if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request) {
                    status.available = false;
                    status.error = Some(format!("HTTP error: {e}"));
                    status.last_check = chrono::Utc::now();
                }
            }

            cleanup_connection_on_exit(
                &connection_id,
                &connection_tracker,
                &event_sender,
                "http_error",
            )
            .await;
            Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("HTTP error"))?)
        }
    };

    result
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
            if let Ok(mut state_guard) = state.lock() {
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
    let diagnostics = if let Ok(tracker) = connection_tracker.lock() {
        tracker.get_connection_diagnostics()
    } else {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("Failed to access connection tracker"))?);
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
    let state_guard = match state.lock() {
        Ok(guard) => guard,
        Err(e) => {
            tracing::error!("Failed to acquire state lock: {}", e);
            return Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal server error"))?);
        }
    };

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
