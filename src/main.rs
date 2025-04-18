#![cfg_attr(coverage, feature(coverage_attribute))]

use std::collections::HashMap;
use std::sync::Arc;

use base64::prelude::*;
use clap::Parser;
use log::{debug, error, info, warn, LevelFilter};
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaChaRng;
#[cfg(feature = "webspy")] use rocket::fs::{FileServer,relative};
use rocket::futures::channel::{self, mpsc::UnboundedSender};
use rocket::futures::future::Either;
use rocket::futures::StreamExt;
use rocket::http::Status;
use rocket::request::FromRequest;
use rocket::response::content::RawJson;
use rocket::response::stream::{Event, EventStream};
use rocket::serde::json::Json;
use rocket::tokio::time::Duration;
use rocket::{catch, catchers, launch, post, routes, Build, Request, Rocket};
use rocket::State;
use rocket_cors::{AllowedOrigins, CorsOptions};
use serde::{Deserialize, Serialize};
use shvclient::client::{CallRpcMethodError, CallRpcMethodErrorKind};
use shvclient::{ClientEvent, ConnectionFailedKind};
use shvproto::RpcValue;
use shvrpc::rpc::ShvRI;
use shvrpc::RpcMessageMetaTags;
use simple_logger::SimpleLogger;
use tokio::sync::{Mutex, RwLock};
use url::Url;

#[cfg(test)] mod tests;

type ClientCommandSender = shvclient::ClientCommandSender<()>;

async fn start_client(config: shvrpc::client::ClientConfig) -> Option<(ClientCommandSender, shvclient::ClientEventsReceiver)> {
    let (tx, rx) = rocket::futures::channel::oneshot::channel();
    tokio::spawn(async move {
        shvclient::client::Client::new_plain()
            .run_with_init(&config, |commands_tx, events_rx|
                tx.send((commands_tx, events_rx))
                .unwrap_or_else(|(commands_tx, _)| {
                    warn!("Client channels dropped before handed to the caller. Terminating the client");
                    commands_tx.terminate_client();
                })
            )
            .await
            .unwrap_or_else(|e| error!("Client finished with error: {e}"));
        }
    );
    rx.await.ok()
}

type ErrorResponse = (Status, Json<ErrorResponseBody>);

#[derive(Clone,Debug,Deserialize,Serialize)]
#[cfg_attr(test, derive(PartialEq))]
struct ErrorResponseBody {
    code: u16,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    shv_error: Option<String>,
}

fn err_response<T: AsRef<str>>(status: Status, detail: impl Into<Option<T>>) -> ErrorResponse {
    (
        status,
        Json(ErrorResponseBody {
            code: status.code,
            detail: detail.into().map_or_else(|| "Unspecified reason".to_string(), |v| v.as_ref().to_string()),
            shv_error: None,
        })
    )
}

#[derive(Deserialize)]
struct SubscribeRequest<'t> {
    shv_ri: &'t str,
}

#[derive(shvproto::TryFromRpcValue)]
#[cfg_attr(test, derive(Debug, PartialEq))]
struct SubscribeEvent {
    path: Option<String>,
    signal: Option<String>,
    param: Option<RpcValue>,
}


#[post("/subscribe", data = "<request>")]
async fn api_subscribe(
    session: Session,
    request: Result<Json<SubscribeRequest<'_>>, rocket::serde::json::Error<'_>>,
) -> Result<EventStream![], ErrorResponse>
{
    let Session(_session_id, SessionData { command_channel, session_channel, .. }) = session;
    let Json(SubscribeRequest { shv_ri }) = request
        .map_err(|e| err_response(Status::UnprocessableEntity, e.to_string()))?;
    let shv_ri = ShvRI::try_from(shv_ri)
        .map_err(|e| err_response(Status::UnprocessableEntity, e))?;
    let mut subscriber = command_channel
        .subscribe(shv_ri)
        .await
        .map_err(|e| err_response(Status::InternalServerError, e.to_string()))?;

    struct UnsubscribeNotifier(UnboundedSender<SessionEvent>);
    impl Drop for UnsubscribeNotifier {
        fn drop(&mut self) {
            self.0.unbounded_send(SessionEvent::Unsubscription)
                .unwrap_or_else(|e| error!("Cannot send SessionEvent::Unsubscription: {e}"));
        }
    }

    session_channel
        .unbounded_send(SessionEvent::Subscription)
        .unwrap_or_else(|e| error!("Cannot send SessionEvent::Subscription: {e}"));

    let event_stream = EventStream! {
        // Notify the session task when the EventStream finishes
        let _notifier = UnsubscribeNotifier(session_channel);
        loop {
            match subscriber.next().await {
                None => break,
                Some(frame) => {
                    match frame.to_rpcmesage() {
                        Err(e) => {
                            warn!("Received invalid RPC frame in notification: {e}\nframe: {frame}");
                            yield Event::data(e.to_string()).event("error");
                        }
                        Ok(msg) => yield Event::data(
                            RpcValue::from(SubscribeEvent{
                                path: msg.shv_path().map(String::from),
                                signal: msg.method().map(String::from),
                                param: msg.param().cloned(),
                            })
                            .to_json()
                        ),
                    }

                }
            }
        }
    };
    Ok(event_stream)
}

#[derive(Deserialize, Serialize)]
#[cfg_attr(test, derive(Debug, PartialEq))]
struct LoginResponse {
    session_id: String,
}

#[post("/login", data = "<params>")]
async fn api_login(
    params: Result<Json<LoginParams<'_>>, rocket::serde::json::Error<'_>>,
    program_config: &State<ProgramConfig>,
    sessions: &State<Sessions>,
    random: &State<Random>,
) -> Result<Json<LoginResponse>, ErrorResponse>
{
    let params = params
        .map_err(|e| err_response(Status::UnprocessableEntity, e.to_string()))?;
    let mut url = program_config.broker_url.clone();
    url.set_username(params.username)
        .map_err(|()| {
            error!("Cannot set username {} for URL {}", params.username, url);
            err_response(Status::InternalServerError, "Cannot authenticate")
        })?;
    url.set_password(Some(params.password))
        .map_err(|()| {
            error!("Cannot set password {} for URL {}", params.password, url);
            err_response(Status::InternalServerError, "Cannot authenticate")
        })?;
    let heartbeat_interval = program_config.heartbeat_interval;
    let client_config = shvrpc::client::ClientConfig { url, heartbeat_interval, ..Default::default() };

    let (client_commands_tx, mut client_events_rx) = start_client(client_config)
        .await
        .ok_or_else(|| {
            warn!("Cannot start SHV client for user `{}`", params.username);
            err_response(Status::InternalServerError, "Client task failure")
        })?;

    // Wait for the client to connect
    match client_events_rx.next().await {
        Some(ClientEvent::Connected(_)) => { }
        None | Some(ClientEvent::Disconnected) | Some(ClientEvent::ConnectionFailed(ConnectionFailedKind::NetworkError)) => {
            return Err(err_response(Status::ServiceUnavailable, "Connection to the broker failed"));
        }
        Some(ClientEvent::ConnectionFailed(ConnectionFailedKind::LoginFailed)) => {
            return Err(err_response(Status::Unauthorized, "Bad credentials"));
        }
    }

    // Generate a new session ID
    let Random(random) = random.inner();
    let mut random_bytes = vec![0u8;30];
    random.lock().await.fill_bytes(&mut random_bytes);
    let session_id = BASE64_URL_SAFE.encode(random_bytes);


    // Check the user sessions count limit for this user.
    // The check is deliberately conducted *after* authentication succeeds to
    // ensure that unauthorized individuals cannot determine whether the user
    // has reached the limit.
    // The check and the write to the sessions store is performed atomically to
    // avoid a race condition when more clients connect simultaneously.
    let Sessions(sessions) = sessions.inner();
    let mut sessions_wr = sessions.write().await;
    let user_sessions_count = sessions_wr
        .values()
        .filter(|SessionData { username, .. }| username == params.username)
        .count() as i32;
    if user_sessions_count == program_config.max_user_sessions {
        client_commands_tx.terminate_client();
        return Err(err_response(Status::Forbidden, "Maximum number of sessions for the user exceeded"));
    }
    let (session_tx, mut session_rx) = channel::mpsc::unbounded();
    // Save the session
    sessions_wr.insert(
        session_id.clone(),
        SessionData {
            command_channel: client_commands_tx,
            session_channel: session_tx,
            username: params.username.into(),
        });
    drop(sessions_wr);

    // Spawn the session task, which maintains the timeout and removes the session when the client terminates
    {
        let session_timeout = program_config.session_timeout;
        let new_session_timer = move || Box::pin(Either::Left(tokio::time::sleep(session_timeout)));
        let disabled_session_timer = || Box::pin(Either::Right(std::future::pending()));

        let sessions = sessions.clone();
        let session_id = session_id.clone();
        tokio::spawn(async move {
            let mut session_timer = new_session_timer();
            let mut subscriptions_count = 0_i64;
            loop {
                tokio::select! {
                    _ = &mut session_timer => {
                        // The session has timed out
                        if let Some(SessionData { command_channel, username, .. }) = sessions.read().await.get(&session_id) {
                            info!("Session {session_id} for user {username} has timed out");
                            command_channel.terminate_client();
                        }
                        session_timer = disabled_session_timer();
                    }
                    client_event = client_events_rx.next() => match client_event {
                        Some(ClientEvent::Connected(_)) => continue,
                        _ => {
                            if let Some(SessionData { username, .. }) = sessions.write().await.remove(&session_id) {
                                info!("Session {session_id} for user {username} has been removed");
                            }
                            break;
                        }
                    },
                    session_event = &mut session_rx.select_next_some() => match session_event {
                        SessionEvent::Activity => {
                            // Reset the timer unless there is an active subscription, in which
                            // case the timer is disabled.
                            if subscriptions_count == 0 {
                                session_timer = new_session_timer();
                            }
                            debug!("activity, subscriptions count: {subscriptions_count}");
                        }
                        SessionEvent::Subscription => {
                            if subscriptions_count == 0 {
                                session_timer = disabled_session_timer();
                            }
                            subscriptions_count += 1;
                            debug!("+subscription: {subscriptions_count}");
                        },
                        SessionEvent::Unsubscription => {
                            subscriptions_count -= 1;
                            if subscriptions_count == 0 {
                                session_timer = new_session_timer();
                            }
                            debug!("-subscription: {subscriptions_count}");
                        },
                    }
                }
            }
        });
    }

    Ok(Json(LoginResponse { session_id }))
}

#[derive(Deserialize)]
struct LoginParams<'r> {
    username: &'r str,
    password: &'r str,
}

enum SessionEvent {
    Activity,
    Subscription,
    Unsubscription,
}

#[derive(Clone)]
struct SessionData {
    command_channel: ClientCommandSender,
    session_channel: UnboundedSender<SessionEvent>,
    username: String,
}

#[derive(Clone, Default)]
struct Sessions(pub(crate) Arc<RwLock<HashMap<String, SessionData>>>);


#[post("/logout")]
async fn api_logout(session: Session) {
    let Session(_, SessionData { command_channel, username, .. }) = session;
    info!("Logout session of user `{username}`");
    command_channel.terminate_client();
}

#[derive(shvproto::TryFromRpcValue)]
struct RpcRequest {
    path: String,
    method: String,
    param: Option<RpcValue>,
}

fn err_response_rpc_call(e: CallRpcMethodError) -> ErrorResponse {
    (
        Status::InternalServerError,
        Json(ErrorResponseBody {
            code: Status::InternalServerError.code,
            shv_error: Some(match e.error() {
                CallRpcMethodErrorKind::ConnectionClosed => "ConnectionClosed".to_string(),
                CallRpcMethodErrorKind::InvalidMessage(_) => "InvalidMessage".to_string(),
                CallRpcMethodErrorKind::RpcError(rpc_err) => format!("RpcError({})", rpc_err.code),
                CallRpcMethodErrorKind::ResultTypeMismatch(_) => "ResultTypeMismatch".to_string(),
            }),
            detail: e.to_string(),
        })
    )
}

macro_rules !return_err {
    ($req:ident, $status:expr, $detail:expr) => {
        {
            let e = err_response($status, $detail);
            $req.local_cache(|| e.clone());
            return Outcome::Error(($status, e));
        }
    };
}

struct RpcValueJson<T>(T);

#[rocket::async_trait]
impl<'r, T> rocket::data::FromData<'r> for RpcValueJson<T>
where T: TryFrom<RpcValue>,
      T::Error: std::fmt::Display,
{
    type Error = ErrorResponse;

    async fn from_data(req: &'r Request<'_>, data: rocket::Data<'r>) -> rocket::data::Outcome<'r, Self> {
        use rocket::data::Outcome;
        if req.content_type() != Some(&rocket::http::ContentType::JSON) {
            return_err!(req, Status::UnsupportedMediaType, "Expected Content-Type: application/json");
        }

        let limit = req.limits().get("json").unwrap_or(rocket::data::Limits::JSON);

        let string = match data.open(limit).into_string().await {
            Ok(string) if string.is_complete() => string.into_inner(),
            Ok(_) => return_err!(req, Status::PayloadTooLarge, "Payload too large"),
            Err(e) => return_err!(req, Status::InternalServerError, format!("{e}")),
        };

        let value = match RpcValue::from_json(&string) {
            Ok(v) => v,
            Err(err) => return_err!(req, Status::UnprocessableEntity, format!("Cannot parse JSON to RpcValue: {err}")),
        };

        let res: T = match value.try_into() {
            Ok(v) => v,
            Err(err) => return_err!(req, Status::UnprocessableEntity, format!("Cannot convert RpcValue to the target type: {err}")),
        };

        Outcome::Success(RpcValueJson(res))
    }
}

#[post("/rpc", data = "<request>")]
async fn api_rpc(session: Session, request: RpcValueJson<RpcRequest>) -> Result<RawJson<String>, ErrorResponse> {
    let Session(_, SessionData { command_channel, session_channel, .. }) = session;
    session_channel
        .unbounded_send(SessionEvent::Activity)
        .unwrap_or_else(|e| error!("Cannot send SessionEvent::Activity: {e}"));
    let RpcValueJson(request) = request;
    let result: shvproto::RpcValue = command_channel
        .call_rpc_method(&request.path, &request.method, request.param)
        .await
        .map_err(err_response_rpc_call)?;
    Ok(RawJson(result.to_json()))
}

struct Session(String, SessionData);

#[rocket::async_trait]
impl<'r> FromRequest<'r> for Session {
    type Error = ErrorResponse;


    async fn from_request(req: &'r Request<'_>) -> rocket::request::Outcome<Self, Self::Error> {
        use rocket::request::Outcome;
        let value = req
            .headers()
            .get_one("Authorization");
        let Some(session_id) = value else {
            return_err!(req, Status::BadRequest, "Missing Authorization header");
        };

        let Sessions(sessions) = req.rocket().state().expect("Sessions are present");
        let Some(session_data) = sessions.read().await.get(session_id).cloned() else {
            return_err!(req, Status::Unauthorized, "Invalid session token");
        };

        Outcome::Success(Session(session_id.into(), session_data))
    }
}

#[catch(default)]
fn catch_default(status: Status, req: &Request) -> ErrorResponse {
    req.local_cache(|| err_response::<&str>(status, status.reason())).clone()
}

struct Random(pub(crate) Arc<Mutex<ChaChaRng>>);

#[derive(Debug, clap::Parser)]
struct ProgramConfig {
    #[arg(long)]
    broker_url: Url,
    #[arg(long, default_value = "10")]
    max_user_sessions: i32,
    #[arg(long, default_value = "10m", value_parser = |val: &str| duration_str::parse_std(val))]
    session_timeout: Duration,
    #[arg(long, default_value = "60s", value_parser = |val: &str| duration_str::parse_std(val))]
    heartbeat_interval: Duration,
    #[arg(short = 'v', long = "verbose")]
    verbose: Option<String>,
    #[arg(short = 'V', long = "version")]
    version: bool,
}

fn init_logger(program_config: &ProgramConfig) {
    let mut logger = SimpleLogger::new();
    logger = logger.with_level(LevelFilter::Info);
    if let Some(module_names) = &program_config.verbose {
        for (module, level) in shvproto::util::parse_log_verbosity(module_names, module_path!()) {
            if let Some(module) = module {
                logger = logger.with_module_level(module, level);
            } else {
                logger = logger.with_level(level);
            }
        }
    }
    logger.init().unwrap();
}

fn print_banner(text: impl AsRef<str>) {
    let text = text.as_ref();
    let width = text.len() + 2;
    let banner_line = format!("{:=^width$}", "");
    info!("{banner_line}");
    info!("{text:^width$}");
    info!("{banner_line}");
}

#[launch]
fn rocket() -> _ {
    static PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
    let program_config = ProgramConfig::parse();

    if program_config.version {
        println!("{PKG_VERSION}");
        std::process::exit(0);
    }

    init_logger(&program_config);

    print_banner(format!("{} {} starting", std::module_path!(), PKG_VERSION));
    info!("{program_config:?}");

    build_rocket(program_config)
}

pub(crate) fn build_rocket(program_config: ProgramConfig) -> Rocket<Build> {
    let cors = CorsOptions::default()
        .allowed_origins(AllowedOrigins::all())
        .allowed_methods(
            [rocket::http::Method::Post]
            .into_iter()
            .map(From::from)
            .collect(),
        )
        .allow_credentials(false);

    let rocket = rocket::build()
        .configure(rocket::Config {
            // We are using a custom logger implementation
            log_level: rocket::config::LogLevel::Off,
            ..Default::default()
        })
        .attach(cors.to_cors().expect("Cannot set CORS policy"))
        .mount("/api", routes![api_login, api_logout, api_rpc, api_subscribe])
        .register("/", catchers![catch_default])
        .manage(program_config)
        .manage(Sessions::default())
        .manage(Random(Arc::new(Mutex::new(ChaChaRng::from_os_rng()))));

    #[cfg(feature = "webspy")]
    let rocket = rocket.mount("/webspy", FileServer::from(relative!("webspy/dist")));

    rocket
}
