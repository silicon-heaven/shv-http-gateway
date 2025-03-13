#![cfg_attr(coverage, coverage(off))]

use std::future::Future;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use const_format::formatcp;
use log::{error, info, warn};
use rocket::futures::future::join_all;
use rocket::futures::stream::FuturesUnordered;
use rocket::futures::StreamExt;
use rocket::http::ContentType;
use rocket::local::asynchronous::Client as RocketClient;
use rocket::http::Status;
use shvclient::appnodes::DotAppNode;
use shvclient::client::{CallRpcMethodError, CallRpcMethodErrorKind};
use shvclient::{ClientCommandSender, ClientEventsReceiver};
use shvproto::RpcValue;
use shvrpc::client::ClientConfig;
use shvrpc::rpcmessage::RpcError;
use tokio_util::compat::TokioAsyncReadCompatExt;
use url::Url;

use crate::{build_rocket, ErrorResponseBody, LoginResponse, ProgramConfig, SubscribeEvent};

const BROKER_ADDRESS: &str = "127.0.0.1:37567";
const BROKER_URL: &str = formatcp!("tcp://{BROKER_ADDRESS}");
const BROKER_URL_WITH_CREDENTIALS: &str = formatcp!("tcp://admin:admin@{BROKER_ADDRESS}");

async fn start_broker() {
    let mut broker_config = shvbroker::config::BrokerConfig::default();
    broker_config.listen.tcp = Some(BROKER_ADDRESS.into());
    let access_config = broker_config.access.clone();
    tokio::spawn(async {
        shvbroker::brokerimpl::accept_loop(broker_config, access_config, None)
            .await
            .expect("broker accept_loop failed")
    });
    // Wait for the broker
    let start = tokio::time::Instant::now();
    while start.elapsed() < tokio::time::Duration::from_secs(5) {
        if tokio::net::TcpStream::connect(BROKER_ADDRESS).await.is_ok() {
            return;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
    panic!("Could not start the broker");
}

async fn start_testing_client() -> Option<(ClientCommandSender, ClientEventsReceiver)> {
    let (tx, rx) = rocket::futures::channel::oneshot::channel();
    tokio::spawn(async {
        let client_config = ClientConfig {
            url: Url::parse(BROKER_URL_WITH_CREDENTIALS).unwrap(),
            device_id: Some("test-device".into()),
            mount: None,
            heartbeat_interval: Duration::from_secs(60),
            reconnect_interval: None,
        };
        shvclient::client::Client::<_,()>::new(DotAppNode::new("testing_client"))
            .mount("value", shvclient::fixed_node! (
                    value_node(request, _tx) {
                        "echo" [IsGetter, Read, "", ""] (param: RpcValue) => {
                            Some(Ok(param))
                        }
                    })
            )
            .run_with_init(&client_config, |commands_tx, events_rx| {
                {
                    let commands_tx = commands_tx.clone();
                    let mut events_rx = events_rx.clone();
                    tokio::spawn(async move {
                        match events_rx.wait_for_event().await {
                            Ok(shvclient::ClientEvent::Connected(_)) => { }
                            _ => return,
                        }
                        let mut interval = tokio::time::interval(Duration::from_millis(100));
                        loop {
                            interval.tick().await;
                            let sig = shvrpc::RpcMessage::new_signal("value", "event", Some(42.into()));
                            commands_tx.send_message(sig).unwrap_or_else(|_| error!("Cannot send signal"));
                        }
                    });
                }
                tx.send((commands_tx, events_rx))
                .unwrap_or_else(|(commands_tx, _)| {
                    warn!("Client channels dropped before handed to the caller. Terminating the client");
                    commands_tx.terminate_client();
                })
            }
            )
            .await
            .unwrap_or_else(|e| error!("Client finished with error: {e}"));
        }
    );
    rx.await.ok()
}

// Define common runtime for the tests.
//
// #[tokio::test] cannot be used, because it creates a new runtime per test. When the first test
// calls `setup()`, the broker runs on that test’s runtime, which stops when the test ends. This
// also cancels the broker task, causing subsequent tests to fail.
// Instead, `shared_rt_test()` runs each test in a shared runtime, ensuring the broker task stays
// alive for the entire lifespan of all tests.
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Cannot build a runtime");
    rt.block_on(setup());
    rt
});

async fn setup() {
    simple_logger::SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init().unwrap();

    start_broker().await;

    let (_c, mut e)  = start_testing_client().await.unwrap();
    let res = e.wait_for_event().await.unwrap();
    match res {
        shvclient::ClientEvent::Connected(_) => {}
        _ => panic!("Testing device cannot connect to the broker"),
    }
}

fn shared_rt_test(test_fut: impl Future<Output = ()>) {
    RUNTIME.block_on(test_fut);
}


fn program_config() -> ProgramConfig {
    ProgramConfig {
        broker_url: Url::parse(BROKER_URL).unwrap(),
        max_user_sessions: 10,
        session_timeout: Duration::from_secs(60),
        heartbeat_interval: Duration::from_secs(60),
        verbose: None,
        version: false,
    }
}

#[tokio::test]
async fn err_response() {
    let (error_status, error_body) = crate::err_response(Status::BadRequest, "Invalid input");
    assert_eq!(error_status, Status::BadRequest);
    let body: ErrorResponseBody = serde_json::from_str(&error_body).unwrap();
    assert_eq!(body.code, Status::BadRequest.code);
    assert_eq!(body.detail, "Invalid input");
}

#[tokio::test]
async fn err_rpc_response() {
    {
        let (error_status, error_body) = crate::err_response_rpc_call(
            CallRpcMethodError::new(
                "foo/bar",
                "baz",
                CallRpcMethodErrorKind::RpcError(RpcError::new(
                        shvrpc::rpcmessage::RpcErrorCode::MethodNotFound,
                        "Unknown method")
                )
            )
        );
        assert_eq!(error_status, Status::InternalServerError);
        let body: ErrorResponseBody = serde_json::from_str(&error_body).unwrap();
        assert_eq!(body.code, Status::InternalServerError.code);
        assert_eq!(body.shv_error, Some("RpcError(MethodNotFound)".into()));
    }
    {
        let (error_status, error_body) = crate::err_response_rpc_call(
            CallRpcMethodError::new(
                "foo/bar",
                "baz",
                CallRpcMethodErrorKind::ConnectionClosed)
        );
        assert_eq!(error_status, Status::InternalServerError);
        let body: ErrorResponseBody = serde_json::from_str(&error_body).unwrap();
        assert_eq!(body.code, Status::InternalServerError.code);
        assert_eq!(body.shv_error, Some("ConnectionClosed".into()));
    }
}

#[tokio::test]
async fn api_login_invalid_request() {
    let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
    let response = client.post("/api/login").dispatch().await;
    assert_eq!(response.status(), Status::UnprocessableEntity);
}

#[tokio::test]
async fn api_rpc_missing_auth_header() {
    let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
    let response = client
        .post("/api/rpc")
        .header(ContentType::JSON)
        .dispatch().await;
    assert_eq!(response.status(), Status::BadRequest);
}

#[test]
fn api_login_passes() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "admin", "password": "admin"}"#)
            .dispatch()
            .await;
        assert_eq!(resp.status(), Status::Ok);
    });
}

#[test]
fn api_login_fails() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "whoa", "password": "idk"}"#)
            .dispatch()
            .await;
        assert_eq!(resp.status(), Status::Unauthorized);
        assert_eq!(ErrorResponseBody {
            code: Status::Unauthorized.code,
            detail: "Bad credentials".into(),
            shv_error: None,
        },
        resp.into_json::<ErrorResponseBody>().await.unwrap());
    });
}

#[test]
fn api_login_and_logout() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();

        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "admin", "password": "admin"}"#)
            .dispatch()
            .await;
        assert_eq!(resp.status(), Status::Ok);
        let session_id = resp.into_json::<LoginResponse>().await.unwrap().session_id;

        let resp = client
            .post("/api/logout")
            .header(rocket::http::Header::new("Authorization", session_id))
            .dispatch()
            .await;
        assert_eq!(resp.status(), Status::Ok);
    });
}

// NOTE: This test works only if the user used here is not shared with other tests!
#[test]
fn api_login_max_sessions_exceeds() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
        let req = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "test", "password": "test"}"#);
        let num_of_passing = program_config().max_user_sessions;
        let num_of_failing = 5;
        let res = (1..=(num_of_passing + num_of_failing)).map(|_| req.clone().dispatch())
            .collect::<FuturesUnordered<_>>()
            .collect::<Vec<_>>()
            .await;
        let num_ok_responses = res.iter().filter(|r| r.status() == Status::Ok).count();
        let num_err_responses = res.iter().filter(|r| r.status() == Status::Forbidden).count();
        assert_eq!(num_ok_responses as i32, num_of_passing);
        assert_eq!(num_err_responses as i32, num_of_failing);
    });
}

#[test]
fn api_rpc_calls() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();

        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "admin", "password": "admin"}"#)
            .dispatch()
            .await;
        let session_id = resp.into_json::<LoginResponse>().await.unwrap().session_id;

        struct RpcCallDispatcher {
            client: RocketClient,
            session_id: String,
        }
        impl RpcCallDispatcher {
            async fn call(&self, path: impl AsRef<str>, method: impl AsRef<str>, param: Option<impl Into<RpcValue>>) -> rocket::local::asynchronous::LocalResponse<'_> {
                let path = path.as_ref();
                let method = method.as_ref();
                let body = if let Some(param) = param {
                    format!(r#"{{"path": "{path}", "method": "{method}", "param": {}}}"#, param.into().to_json())
                } else {
                    format!(r#"{{"path": "{path}", "method": "{method}"}}"#)
                };
                info!("RpcCall body: {body}");
                self.client
                    .post("/api/rpc")
                    .header(rocket::http::Header::new("Authorization", self.session_id.clone()))
                    .header(ContentType::JSON)
                    .body(body)
                    .dispatch()
                    .await
            }
        }

        // Wrong session token
        {
            let resp = client
                .post("/api/rpc")
                .header(rocket::http::Header::new("Authorization", "wrong_token"))
                .header(ContentType::JSON)
                .body(r#"{"path": ".broker", "method": "anything"}"#)
                .dispatch()
                .await;
            assert_eq!(resp.status(), Status::Unauthorized);
        }

        // Wrong body format
        {
            let resp = client
                .post("/api/rpc")
                .header(rocket::http::Header::new("Authorization", session_id.clone()))
                .header(ContentType::JSON)
                .body(r#"{"abc": "def"}"#)
                .dispatch()
                .await;
            assert_eq!(resp.status(), Status::UnprocessableEntity);
        }

        // Regular calls
        let rpc_call_dispatcher = RpcCallDispatcher { client, session_id };
        {
            let resp = rpc_call_dispatcher.call(".broker", "ls", None::<RpcValue>).await;
            assert_eq!(resp.status(), Status::Ok);
            let result_rpcval = resp.into_string().await.map(|s| RpcValue::from_json(&s)).unwrap().unwrap();
            assert!(result_rpcval.is_list());
            assert!(!result_rpcval.as_list().is_empty(), ".broker:ls should return non-empty list (got: {result_rpcval:?})");
        }

        let values = [
            RpcValue::from(42),
            RpcValue::from("test"),
            RpcValue::from(shvproto::make_imap!(1 => "foo", 2 => 42))
        ];
        for arg in &values {
            let resp = rpc_call_dispatcher.call("test/device/value", "echo", Some(arg)).await;
            assert_eq!(resp.status(), Status::Ok);
            let result_rpcval = resp.into_string().await.map(|s| RpcValue::from_json(&s)).unwrap().unwrap();
            info!("value:echo sent:{arg} received:{result_rpcval} {}", result_rpcval.type_name());
            assert_eq!(result_rpcval.type_name(), arg.type_name());
            assert_eq!(&result_rpcval, arg, "value:read should return {arg})");
        }
    });
}

#[test]
fn api_subscribe() {
    shared_rt_test(async {
        let client = Arc::new(RocketClient::untracked(build_rocket(program_config())).await.unwrap());

        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "admin", "password": "admin"}"#)
            .dispatch()
            .await;
        let session_id = resp.into_json::<LoginResponse>().await.unwrap().session_id;

        let mut tasks = vec![];
        for task_id in 0..10 {
            let client = client.clone();
            let session_id = session_id.clone();
            let task = tokio::spawn(async move {
                let req = client
                    .post("/api/subscribe")
                    .header(ContentType::JSON)
                    .header(rocket::http::Header::new("Authorization", session_id))
                    .body(format!(r#"{{"shv_ri": "{}:*:*"}}"#,
                            if task_id < 5 { "test/device/value" } else { "test/*" }
                    ))
                    .dispatch();
                let resp = req.await;

                assert!(resp.content_type().unwrap().is_event_stream());

                // let mut reader = tokio::io::BufReader::new(resp).lines();
                // for i in 0..5 {
                //     warn!("receiving event {i}");
                //     let event = reader
                //         .next_line()
                //         .await
                //         .expect("Read line error")
                //         .expect("Unexpected end of stream");
                //     warn!("{event}");
                // }

                let mut reader = sse_codec::decode_stream(tokio::io::BufReader::new(resp).compat());
                for i in 0..10 {
                    info!("task {task_id}, receiving event {i}");
                    let event = reader
                        .next()
                        .await
                        .expect("Unexpected end of stream")
                        .unwrap_or_else(|e| panic!("Unexpected error in event stream: {e}"));
                    let sse_codec::Event::Message{ id, event, data} = event else {
                        panic!("Unexpected event");
                    };
                    info!("{data}");
                    assert!(id.is_none());
                    assert_eq!(event, "message");
                    let parsed_data: SubscribeEvent = RpcValue::from_json(&data).unwrap().try_into().unwrap();
                    assert_eq!(parsed_data.path, Some("test/device/value".into()));
                    assert_eq!(parsed_data.signal, Some("event".into()));
                    assert_eq!(parsed_data.param, Some(RpcValue::from(42)));
                }
            });
            tasks.push(task);
        }
        join_all(tasks).await;
    });
}
