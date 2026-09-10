use std::{env, io::Read, net::IpAddr, path::PathBuf, process::Stdio, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use calc_api::calculator::{CalcError, CalcRequest, CalcResponse, evaluate};
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::Semaphore,
    time::timeout,
};

const MAX_BODY: usize = 32 * 1024;
const MAX_OUTPUT: u64 = 2 * 1024 * 1024;

#[derive(Clone)]
struct AppState {
    workers: Arc<Semaphore>,
    deadline: Duration,
    executable: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if env::args().nth(1).as_deref() == Some("--worker") {
        return worker();
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(serve())
}

fn worker() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    limit_worker_memory()?;
    // Never let an upstream panic contaminate the HTTP server or its output.
    std::panic::set_hook(Box::new(|_| {}));
    let mut input = Vec::new();
    std::io::stdin()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut input)?;
    if input.len() > MAX_BODY {
        return Err("worker input too large".into());
    }
    let request: CalcRequest = serde_json::from_slice(&input)?;
    let result = std::panic::catch_unwind(|| evaluate(request)).unwrap_or_else(|_| {
        Err(calc_error(
            "evaluation_failed",
            "The calculation engine could not evaluate this expression.",
        ))
    });
    serde_json::to_writer(std::io::stdout().lock(), &result)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn limit_worker_memory() -> std::io::Result<()> {
    let memory = libc::rlimit {
        rlim_cur: 256 * 1024 * 1024,
        rlim_max: 256 * 1024 * 1024,
    };
    let cores = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: these pointers reference valid rlimit structs and only configure this worker.
    if unsafe { libc::setrlimit(libc::RLIMIT_AS, &memory) } != 0
        || unsafe { libc::setrlimit(libc::RLIMIT_CORE, &cores) } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn setting(name: &str, default: u64, min: u64, max: u64) -> Result<u64, String> {
    let value = match env::var(name) {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|_| format!("{name} must be an integer"))?,
        Err(env::VarError::NotPresent) => default,
        Err(_) => return Err(format!("{name} must be valid UTF-8")),
    };
    if !(min..=max).contains(&value) {
        return Err(format!("{name} must be between {min} and {max}"));
    }
    Ok(value)
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let host: IpAddr = env::var("HOST")
        .unwrap_or_else(|_| "0.0.0.0".into())
        .parse()?;
    let port = setting("PORT", 8080, 0, 65535)? as u16;
    let state = AppState {
        workers: Arc::new(Semaphore::new(
            setting("CALC_MAX_WORKERS", 4, 1, 32)? as usize
        )),
        deadline: Duration::from_millis(setting("CALC_TIMEOUT_MS", 2000, 1, 30000)?),
        executable: env::current_exe()?,
    };
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
        .route("/calc", post(calculate))
        .fallback(|| async { api_error(StatusCode::NOT_FOUND, "not_found", "Endpoint not found.") })
        .method_not_allowed_fallback(|| async {
            api_error(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed.",
            )
        })
        .with_state(state);
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("calc-api listening on {}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

async fn calculate(State(state): State<AppState>, request: Request) -> Response {
    let media_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .unwrap_or("")
        .trim();
    if !media_type.eq_ignore_ascii_case("application/json") {
        return api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Use Content-Type: application/json.",
        );
    }
    let bytes = match timeout(
        Duration::from_secs(5),
        to_bytes(request.into_body(), MAX_BODY),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) => {
            use std::error::Error;
            let is_limit = error
                .source()
                .is_some_and(|source| source.to_string().contains("length limit"));
            return if is_limit {
                api_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "payload_too_large",
                    "Request body exceeds 32768 bytes.",
                )
            } else {
                api_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Could not read request body.",
                )
            };
        }
        Err(_) => {
            return api_error(
                StatusCode::REQUEST_TIMEOUT,
                "timeout",
                "Request body timed out.",
            );
        }
    };
    let request: CalcRequest = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(error) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                &error.to_string(),
            );
        }
    };
    if let Err(error) = request.validate() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": error}))).into_response();
    }
    let Ok(_permit) = state.workers.try_acquire() else {
        let mut response = api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "busy",
            "All calculation workers are busy. Retry shortly.",
        );
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "1".parse().unwrap());
        return response;
    };
    match run_worker(&state, &request).await {
        Ok(result) => Json(result).into_response(),
        Err(error) => {
            let status = match error.code.as_str() {
                "timeout" => StatusCode::REQUEST_TIMEOUT,
                "invalid_request" => StatusCode::BAD_REQUEST,
                "internal_error" => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            (status, Json(json!({"error": error}))).into_response()
        }
    }
}

async fn run_worker(state: &AppState, request: &CalcRequest) -> Result<CalcResponse, CalcError> {
    let input = serde_json::to_vec(request)
        .map_err(|_| calc_error("internal_error", "Could not encode calculation."))?;
    let mut child = Command::new(&state.executable)
        .arg("--worker")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| calc_error("internal_error", "Could not start calculation worker."))?;
    let work = async {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| calc_error("internal_error", "Worker input unavailable."))?;
        stdin
            .write_all(&input)
            .await
            .map_err(|_| calc_error("evaluation_failed", "Calculation worker stopped."))?;
        drop(stdin);
        let mut output = Vec::new();
        child
            .stdout
            .take()
            .ok_or_else(|| calc_error("internal_error", "Worker output unavailable."))?
            .take(MAX_OUTPUT + 1)
            .read_to_end(&mut output)
            .await
            .map_err(|_| calc_error("evaluation_failed", "Could not read calculation result."))?;
        if output.len() as u64 > MAX_OUTPUT {
            return Err(calc_error(
                "result_too_large",
                "Calculation result exceeds 2 MiB.",
            ));
        }
        let status = child
            .wait()
            .await
            .map_err(|_| calc_error("internal_error", "Could not collect calculation worker."))?;
        if !status.success() {
            return Err(calc_error(
                "evaluation_failed",
                "The calculation engine could not evaluate this expression.",
            ));
        }
        serde_json::from_slice::<Result<CalcResponse, CalcError>>(&output).map_err(|_| {
            calc_error(
                "evaluation_failed",
                "Calculation worker returned an invalid result.",
            )
        })?
    };
    let result = match timeout(state.deadline, work).await {
        Ok(result) => result,
        Err(_) => Err(calc_error(
            "timeout",
            "Calculation exceeded its time limit.",
        )),
    };
    // Kill and reap before releasing the permit, including on output overflow and I/O errors.
    if child.id().is_some() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

fn calc_error(code: &str, message: &str) -> CalcError {
    CalcError {
        code: code.into(),
        message: message.into(),
    }
}

fn api_error(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"error": calc_error(code, message)}))).into_response()
}
