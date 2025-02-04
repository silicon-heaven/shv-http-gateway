use std::future::Future;
use std::sync::LazyLock;
use std::time::Duration;

use const_format::formatcp;
use rocket::futures::stream::FuturesUnordered;
use rocket::futures::StreamExt;
use rocket::http::ContentType;
use rocket::serde::Deserialize;
use rocket::local::asynchronous::Client as RocketClient;
use rocket::http::Status;
use shvclient::{ClientCommandSender, ClientEventsReceiver};
use shvrpc::client::ClientConfig;
use url::Url;

use crate::{build_rocket, start_client, LoginResponse, ProgramConfig, RpcResponse};

const BROKER_ADDRESS: &str = "127.0.0.1:3755";
const BROKER_URL: &str = formatcp!("tcp://{BROKER_ADDRESS}");
const BROKER_URL_WITH_CREDENTIALS: &str = formatcp!("tcp://admin:admin@{BROKER_ADDRESS}");

async fn start_broker() {
    let mut broker_config = shvbroker::config::BrokerConfig::default();
    broker_config.listen.tcp = Some(BROKER_ADDRESS.into());
    let access_config = broker_config.access.clone();
    tokio::spawn(shvbroker::brokerimpl::accept_loop(broker_config, access_config, None));
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

async fn start_testing_device() -> Option<(ClientCommandSender, ClientEventsReceiver)> {
    start_client(ClientConfig {
        url: Url::parse(BROKER_URL_WITH_CREDENTIALS).unwrap(),
        device_id: Some("test".into()),
        mount: None,
        heartbeat_interval: Duration::from_secs(60),
        reconnect_interval: None,
    }).await
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

    let (_c, mut e)  = start_testing_device().await.unwrap();
    let res = e.wait_for_event().await.unwrap();
    match res {
        shvclient::ClientEvent::Connected => {}
        _ => panic!("Testing device cannot connect to the broker"),
    }
}

fn shared_rt_test(test_fut: impl Future<Output = ()>) {
    RUNTIME.block_on(async {
        test_fut.await
    });
}


fn program_config() -> ProgramConfig {
    ProgramConfig {
        broker_url: Url::parse(BROKER_URL).unwrap(),
        max_user_sessions: 10,
        session_timeout: Duration::from_secs(60),
        heartbeat_interval: Duration::from_secs(60),
        verbose: None,
    }
}

#[derive(Deserialize, Debug, PartialEq)]
#[serde(crate = "rocket::serde")]
struct ApiErrorResponse {
    code: u16,
    detail: String,
}

#[test]
fn login_passes() {
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
fn login_fails() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "whoa", "password": "idk"}"#)
            .dispatch()
            .await;
        assert_eq!(resp.status(), Status::Unauthorized);
        assert_eq!(ApiErrorResponse {
            code: Status::Unauthorized.code,
            detail: "Bad credentials".into(),
        },
        resp.into_json::<ApiErrorResponse>().await.unwrap());
    });
}

// NOTE: This test works only if the user used here is not shared with other tests!
#[test]
fn max_sessions_exceeds() {
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
fn rpc_calls() {
    shared_rt_test(async {
        let client = RocketClient::untracked(build_rocket(program_config())).await.unwrap();
        let resp = client
            .post("/api/login")
            .header(ContentType::JSON)
            .body(r#"{"username": "admin", "password": "admin"}"#)
            .dispatch()
            .await;
        let session_id = resp.into_json::<LoginResponse>().await.unwrap().session_id;

        let resp = client
            .post("/api/rpc")
            .header(rocket::http::Header::new("Authorization", session_id))
            .header(ContentType::JSON)
            .body(r#"{"path": ".broker", "method": "ls"}"#)
            .dispatch()
            .await;
        assert_eq!(resp.status(), Status::Ok);
        let result = resp.into_json::<RpcResponse>().await.unwrap().result;
        log::info!("{result}");

    });
}
