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
                            // Use expect here since this is a last resort error handler
                            // and the Response::builder should never fail with valid inputs
                            Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error"))
                                .expect("Failed to build error response"))
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
                            // Use expect here since this is a last resort error handler
                            // and the Response::builder should never fail with valid inputs
                            Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error"))
                                .expect("Failed to build error response"))
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
                            // Use expect here since this is a last resort error handler
                            // and the Response::builder should never fail with valid inputs
                            Ok(Response::builder()
                                .status(StatusCode::INTERNAL_SERVER_ERROR)
                                .body(Body::from("Internal server error"))
                                .expect("Failed to build error response"))
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

/// Retry result that indicates whether to try fallback endpoints
#[derive(Debug)]
#[allow(dead_code)]
enum RetryResult {
    Success(Response<Body>),
    FailedEndpoint(hyper::Error), // This endpoint failed, try others
    FinalError(hyper::Error),     // All retries failed, no fallback needed
}

/// Retry wrapper for HTTP requests with exponential backoff
async fn retry_request<F, Fut>(
    config: &crate::config::RetryConfig,
    endpoint: &str,
    silent_mode: bool,
    request_fn: F,
) -> RetryResult
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Response<Body>, hyper::Error>>,
{
    use crate::logging::{
        log_retry_attempt, log_retry_delay, log_retry_exhausted, log_retry_success,
    };

    if !config.enabled || config.max_attempts <= 1 {
        return match request_fn().await {
            Ok(response) => RetryResult::Success(response),
            Err(error) => RetryResult::FailedEndpoint(error),
        };
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
                return RetryResult::Success(response);
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
                return RetryResult::FailedEndpoint(error);
            }
        }
    }

    // This should never be reached, but handle it gracefully
    if let Some(final_error) = last_error {
        if !silent_mode {
            log_retry_exhausted(endpoint, config.max_attempts, &format!("{:?}", final_error));
        }
        RetryResult::FailedEndpoint(final_error)
    } else {
        // This is truly an exceptional case - all retries failed but no error was captured
        // Log this unusual condition and return a final error to prevent panic
        if !silent_mode {
            log_retry_exhausted(
                endpoint,
                config.max_attempts,
                "Unknown error - no error captured during retries",
            );
        }
        // We need to create a hyper::Error somehow. Use a timeout-style approach.
        // Since we can't directly construct hyper::Error, we'll use FinalError
        // and let the caller handle the missing error case by using their client
        // to generate an appropriate error response.
        RetryResult::FinalError(
            // Create a temporary client and use the existing error generation pattern
            futures::executor::block_on(async {
                let connector = hyper_tls::HttpsConnector::new();
                let client = hyper::Client::builder().build::<_, hyper::Body>(connector);
                match client
                    .request(
                        hyper::Request::get("")
                            .body(hyper::Body::empty())
                            .expect("Failed to create error request"),
                    )
                    .await
                {
                    Err(e) => e,
                    Ok(_) => unreachable!("Request to empty URL should always fail"),
                }
            }),
        )
    }
}

/// Try request with fallback endpoints when the primary endpoint fails
#[allow(clippy::too_many_arguments)]
async fn try_with_fallback_endpoints(
    body_bytes: hyper::body::Bytes,
    state: &SharedState,
    client: &Client<HttpsConnector<hyper::client::HttpConnector>>,
    method: hyper::Method,
    path_and_query: Option<&str>,
    headers_template: &hyper::HeaderMap,
    version: hyper::Version,
    retry_config: &crate::config::RetryConfig,
    silent_mode: bool,
    current_endpoint: &str,
) -> Result<Response<Body>, hyper::Error> {
    use crate::logging::{log_endpoint_switch, log_proxy_error, log_proxy_request};

    // Get all available endpoints from the state, prioritizing healthy ones
    let available_endpoints = {
        let state_guard = match state.lock() {
            Ok(guard) => guard,
            Err(_) => {
                // If state lock is poisoned, return a simple error
                // Use the existing async error generation pattern synchronously
                return Err(futures::executor::block_on(async {
                    let connector = hyper_tls::HttpsConnector::new();
                    let client = hyper::Client::builder().build::<_, hyper::Body>(connector);
                    match client
                        .request(
                            hyper::Request::get("")
                                .body(hyper::Body::empty())
                                .expect("Failed to create error request"),
                        )
                        .await
                    {
                        Err(e) => e,
                        Ok(_) => unreachable!("Request to empty URL should always fail"),
                    }
                }));
            }
        };
        let mut endpoints = Vec::new();

        // First, try the current endpoint (already attempted, but may work with different timing)
        if let Some(token) = state_guard
            .config
            .get_all_endpoints()
            .iter()
            .find(|(_, endpoint, _)| endpoint.url == current_endpoint)
            .map(|(token, _, _)| token)
        {
            endpoints.push((current_endpoint.to_string(), token.clone(), false));
            // false = already tried
        }

        // Then add other healthy endpoints
        for (token, endpoint, _) in state_guard.config.get_all_endpoints() {
            if endpoint.url != current_endpoint {
                if let Some(status) = state_guard.endpoint_status.get(&endpoint.url) {
                    if status.available {
                        endpoints.push((endpoint.url.clone(), token, true)); // true = new attempt
                    }
                }
            }
        }

        // Finally, add unhealthy endpoints as last resort
        for (token, endpoint, _) in state_guard.config.get_all_endpoints() {
            if endpoint.url != current_endpoint {
                let is_already_added = endpoints.iter().any(|(url, _, _)| url == &endpoint.url);
                if !is_already_added {
                    endpoints.push((endpoint.url.clone(), token, true)); // true = new attempt
                }
            }
        }

        endpoints
    };

    let total_timeout = std::time::Duration::from_secs(600); // 10 minutes total
    let start_time = std::time::Instant::now();

    for (endpoint_url, auth_token, is_new_attempt) in available_endpoints {
        // Check if we're running out of total time
        if start_time.elapsed() >= total_timeout {
            if !silent_mode {
                log_proxy_error(&endpoint_url, "Total request timeout exceeded (10 minutes)");
            }
            break;
        }

        // Skip the current endpoint on first iteration (already failed)
        if endpoint_url == current_endpoint && !is_new_attempt {
            continue;
        }

        // Log endpoint switch for new attempts
        if endpoint_url != current_endpoint && !silent_mode {
            log_endpoint_switch(current_endpoint, &endpoint_url, 999999, 0);
        }

        // Build the target URI
        let uri_string = format!("{}{}", endpoint_url, path_and_query.unwrap_or(""));

        let uri: Uri = match uri_string.parse() {
            Ok(uri) => uri,
            Err(_) => continue, // Skip invalid URIs
        };

        // Create request function for this endpoint
        let body_bytes_for_req = body_bytes.clone();
        let client_for_req = client.clone();
        let uri_for_req = uri.clone();
        let mut headers_for_req = headers_template.clone();
        let method_for_req = method.clone();
        let version_for_req = version;
        let auth_token_for_req = auth_token.clone();

        // Update headers for this endpoint
        let host = endpoint_url
            .strip_prefix("https://")
            .or_else(|| endpoint_url.strip_prefix("http://"))
            .unwrap_or(&endpoint_url);
        if let Ok(host_value) = host.parse() {
            headers_for_req.insert("host", host_value);
        }

        // Update authorization header
        headers_for_req.remove("authorization");
        if !auth_token_for_req.is_empty() {
            let auth_value = format!("Bearer {}", auth_token_for_req);
            if let Ok(auth_header) = auth_value.parse() {
                headers_for_req.insert("authorization", auth_header);
            }
        }

        let request_fn = move || {
            let body_bytes = body_bytes_for_req.clone();
            let client = client_for_req.clone();
            let uri = uri_for_req.clone();
            let headers = headers_for_req.clone();
            let method = method_for_req.clone();
            let version = version_for_req;

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

                let new_req = request_builder
                    .body(Body::from(body_bytes))
                    .expect("Failed to build request from valid parts");

                // Set timeout for individual requests (5 minutes each)
                let timeout_duration = std::time::Duration::from_secs(300);
                match tokio::time::timeout(timeout_duration, client.request(new_req)).await {
                    Ok(result) => result,
                    Err(_timeout) => {
                        // Create a timeout error by making a request to invalid URL
                        // Use expect since this is a fallback error construction
                        client
                            .request(
                                hyper::Request::get("")
                                    .body(Body::empty())
                                    .expect("Failed to create error request"),
                            )
                            .await
                    }
                }
            }
        };

        // Log request if it's a new endpoint attempt
        if !silent_mode {
            log_proxy_request(&endpoint_url);
        }

        // Try this endpoint with retry logic
        match retry_request(retry_config, &endpoint_url, silent_mode, request_fn).await {
            RetryResult::Success(response) => {
                // Success! Update the current endpoint in state if different
                if endpoint_url != current_endpoint {
                    if let Ok(mut state_guard) = state.lock() {
                        state_guard.switch_endpoint_silent(endpoint_url.clone());
                    }
                }
                return Ok(response);
            }
            RetryResult::FailedEndpoint(_error) => {
                // This endpoint failed, mark it as failed and try next
                if let Ok(mut state_guard) = state.lock() {
                    if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_url) {
                        status.available = false;
                        status.error = Some(format!("HTTP error: {}", _error));
                        status.last_check = chrono::Utc::now();
                    }
                }
                continue;
            }
            RetryResult::FinalError(error) => {
                // This should not happen in our current implementation
                return Err(error);
            }
        }
    }

    // All endpoints failed
    if !silent_mode {
        log_proxy_error("all-endpoints", "All endpoints failed after retry attempts");
    }

    // Return a generic connection error since all endpoints failed
    // We need to create a hyper error somehow - use the timeout approach
    let client_for_error = client.clone();
    client_for_error
        .request(
            hyper::Request::get("")
                .body(Body::empty())
                .expect("Failed to create error request"),
        )
        .await
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
        // Get detail level from config
        let detail_level = {
            let state_guard = state
                .lock()
                .map_err(|e| anyhow::anyhow!("Failed to acquire state lock: {}", e))?;
            state_guard.config.logging.proxy_detail.clone()
        };

        // Log detailed request information
        let path = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");
        let request_start_time = std::time::Instant::now();

        log_proxy_request_detailed(
            &endpoint_for_request,
            &parts.method,
            path,
            &connection_id,
            Some(&parts.headers),
            if body_bytes.is_empty() {
                None
            } else {
                Some(&body_bytes)
            },
            &detail_level,
        );

        // Store start time for response logging
        parts.extensions.insert(request_start_time);
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
                    // Use expect since this is a fallback error construction
                    client
                        .request(
                            hyper::Request::get("")
                                .body(Body::empty())
                                .expect("Failed to create error request"),
                        )
                        .await
                }
            }
        }
    };

    // Execute request with enhanced retry logic (cross-endpoint fallback)
    let response = retry_request(
        &retry_config,
        &endpoint_for_request,
        silent_mode,
        request_fn,
    )
    .await;

    // Handle response with enhanced fallback logic
    let result = match response {
        RetryResult::Success(mut resp) => {
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

                    // Keep a clone for logging before the body is moved
                    let body_bytes_for_logging = body_bytes.clone();
                    let new_body = Body::from(body_bytes);

                    // Create new response with the consumed body
                    let (response_parts, _) = resp.into_parts();
                    let final_response = Response::from_parts(response_parts, new_body);

                    // Log detailed response information only if not in silent mode
                    if !silent_mode {
                        // Get detail level from config
                        let detail_level = {
                            let state_guard = state.lock().map_err(|e| {
                                anyhow::anyhow!("Failed to acquire state lock: {}", e)
                            })?;
                            state_guard.config.logging.proxy_detail.clone()
                        };

                        // Calculate response time if we stored the start time
                        let response_time = parts
                            .extensions
                            .get::<std::time::Instant>()
                            .map(|start_time| start_time.elapsed());

                        log_proxy_response_detailed(
                            &endpoint_for_request,
                            final_response.status().as_u16(),
                            &connection_id,
                            response_time.map(|d| d.as_millis() as u64).unwrap_or(0),
                            Some(final_response.headers()),
                            Some(&body_bytes_for_logging),
                            &detail_level,
                        );
                    }

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
        RetryResult::FailedEndpoint(_e) => {
            // Primary endpoint failed after retries, try fallback endpoints
            if !silent_mode {
                log_proxy_error(
                    &endpoint_for_request,
                    &format!("Primary endpoint failed, trying fallbacks: {}", _e),
                );
            }

            // Mark the primary endpoint as failed
            if let Ok(mut state_guard) = state.lock() {
                if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request) {
                    status.available = false;
                    status.error = Some(format!("HTTP error: {}", _e));
                    status.last_check = chrono::Utc::now();
                }
            }

            // Try fallback endpoints with cross-endpoint retry
            match try_with_fallback_endpoints(
                body_bytes,
                &state,
                &client,
                parts.method,
                parts.uri.path_and_query().map(|pq| pq.as_str()),
                &parts.headers,
                parts.version,
                &retry_config,
                silent_mode,
                &endpoint_for_request,
            )
            .await
            {
                Ok(mut fallback_resp) => {
                    // Successfully got response from fallback endpoint
                    // Consume the body with timeout
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(300),
                        hyper::body::to_bytes(fallback_resp.body_mut()),
                    )
                    .await
                    {
                        Ok(Ok(body_bytes)) => {
                            // Success with fallback endpoint
                            if let Ok(mut tracker) = connection_tracker.lock() {
                                tracker.update_connection_status(
                                    &connection_id,
                                    ConnectionStatus::Finishing,
                                );
                            }

                            let new_body = Body::from(body_bytes);
                            let (response_parts, _) = fallback_resp.into_parts();
                            let final_response = Response::from_parts(response_parts, new_body);

                            cleanup_connection_on_exit(
                                &connection_id,
                                &connection_tracker,
                                &event_sender,
                                "fallback_success",
                            )
                            .await;
                            Ok(final_response)
                        }
                        Ok(Err(e)) => {
                            // Fallback body consumption error
                            if !silent_mode {
                                log_proxy_error(
                                    "fallback-endpoint",
                                    &format!("Fallback body consumption error: {e}"),
                                );
                            }
                            cleanup_connection_on_exit(
                                &connection_id,
                                &connection_tracker,
                                &event_sender,
                                "fallback_body_error",
                            )
                            .await;
                            Ok(Response::builder()
                                .status(StatusCode::BAD_GATEWAY)
                                .body(Body::from("Fallback body consumption error"))?)
                        }
                        Err(_) => {
                            // Fallback body consumption timeout
                            if !silent_mode {
                                log_proxy_error(
                                    "fallback-endpoint",
                                    "Fallback body consumption timeout",
                                );
                            }
                            cleanup_connection_on_exit(
                                &connection_id,
                                &connection_tracker,
                                &event_sender,
                                "fallback_body_timeout",
                            )
                            .await;
                            Ok(Response::builder()
                                .status(StatusCode::GATEWAY_TIMEOUT)
                                .body(Body::from("Fallback body consumption timeout"))?)
                        }
                    }
                }
                Err(_fallback_error) => {
                    // All endpoints failed (including fallbacks)
                    if !silent_mode {
                        log_proxy_error(
                            "all-endpoints",
                            &format!(
                                "All endpoints failed: primary={}, fallback={}",
                                _e, _fallback_error
                            ),
                        );
                    }

                    cleanup_connection_on_exit(
                        &connection_id,
                        &connection_tracker,
                        &event_sender,
                        "all_endpoints_failed",
                    )
                    .await;
                    Ok(Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(Body::from("All endpoints unavailable"))?)
                }
            }
        }
        RetryResult::FinalError(e) => {
            // This should rarely happen, but handle it gracefully
            if !silent_mode {
                log_proxy_error(
                    &endpoint_for_request,
                    &format!("Final error after retries: {e}"),
                );
            }

            cleanup_connection_on_exit(
                &connection_id,
                &connection_tracker,
                &event_sender,
                "final_error",
            )
            .await;
            Ok(Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from("Request failed"))?)
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
    proxy_handler_with_events_impl(req, state, client, connection_tracker, event_sender, false)
        .await
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
                if let Ok(mut state_guard) = state.lock() {
                    if let Some(status) = state_guard.endpoint_status.get_mut(&endpoint_for_request)
                    {
                        status.available = false;
                        status.error = Some("Request timeout".to_string());
                        status.last_check = chrono::Utc::now();
                    }
                }
                // If lock is poisoned, continue without updating state
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
