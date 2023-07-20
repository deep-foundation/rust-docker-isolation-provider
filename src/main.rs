#![allow(clippy::let_unit_value)] // false positive: https://github.com/SergioBenitez/Rocket/issues/2568

mod script;

use {
    json::value::RawValue,
    moka::future::Cache,
    rocket::{
        response::{content::RawJson, stream::ByteStream},
        serde::json::Json,
        Shutdown, State,
    },
    std::{
        borrow, env,
        sync::atomic::{AtomicUsize, Ordering},
    },
    tokio::{
        select,
        sync::broadcast::{channel, error::RecvError, Sender},
    },
};

use rocket::{get, post, routes};

#[derive(serde::Deserialize)]
pub struct Call<'a> {
    #[serde(borrow, default)]
    head: borrow::Cow<'a, str>,

    #[serde(borrow)]
    main: borrow::Cow<'a, str>,

    #[serde(borrow)]
    args: &'a RawValue,
}

// todo: possible to use in config, it is very easy:
//  - https://rocket.rs/v0.5-rc/guide/configuration/#configuration
//  - https://crates.io/keywords/configuration
const CRATES: &str = "crates";

#[post("/call", data = "<call>")]
async fn call(
    call: Json<Call<'_>>,
    scripts: &State<Scripts>,
    tx: &State<Sender<Vec<u8>>>,
) -> Result<RawJson<String>, script::Error> {
    static COUNT: AtomicUsize = AtomicUsize::new(0);

    async fn unique_rs() -> String {
        format!("{}.rs", COUNT.fetch_add(1, Ordering::SeqCst))
    }

    let file = scripts.cache.entry_by_ref(call.main.as_ref()).or_insert_with(unique_rs()).await;
    let (out, bytes) = script::execute_in(
        (&env::current_dir()?.join(CRATES), &file.into_value()),
        call.into_inner(), // keep formatting
    )
    .await?;

    // A send 'fails' if there are no active subscribers. That's okay.
    let _ = tx.send(bytes);

    Ok(RawJson(out))
}

#[get("/stream")]
fn stream(stream: &State<Sender<Vec<u8>>>, mut end: Shutdown) -> ByteStream![Vec<u8>] {
    let mut rx = stream.subscribe();

    ByteStream! {
        loop {
            yield select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(bytes) => bytes,
                        Err(RecvError::Closed) => break,
                        Err(RecvError::Lagged(_)) => continue,
                    }
                }
                _ = &mut end => break,
            };
        }
    }
}

struct Scripts {
    pub cache: Cache<String, String>,
}

#[rocket::launch]
fn launch() -> _ {
    #[get("/init")]
    fn init() {}

    #[get("/healthz")]
    fn health() -> &'static str {
        "Service is up and running"
    }

    rocket::build()
        .manage(channel::<Vec<u8>>(1024).0)
        .manage(Scripts { cache: Cache::new(8096) })
        .mount("/", routes![init, health, call, stream])
}

// todo: extract into `tests/` folder
//  it may seem excessive because you need to create `lib.rs` as well,
//  which will lead to a loss of minimalism
#[cfg(test)]
mod tests {
    use {
        rocket::{http::Status, local::blocking::Client, uri},
        std::time::Duration,
        tokio::{join, time},
    };

    macro_rules! rusty {
        (($($pats:tt)*) $(-> $ty:ty)? { $body:expr } $(where $args:expr)? ) => {{
            fn __compile_check() {
                 fn main($($pats)*) $(-> $ty)? { $body }
            }
            json::json!({
                "main": stringify!(
                    fn main($($pats)*) $(-> $ty)? { $body }
                ),
                $("args": $args)?
            })
        }};
    }

    #[test]
    fn rusty() {
        fn clean(json: json::Value) -> String {
            json.to_string().replace(char::is_whitespace, "").replace("\\n", "")
        }

        let raw = json::json!({
            "main": r#"
                fn main(hello: &str) -> String {
                    format!("{hello}world")
                }"#,
            "args": "Hi"
        });

        let rusty = rusty! {
            (hello: &str) -> String {
                format!("{hello} world")
            } where { "Hi" }
        };

        assert_eq!(clean(raw), clean(rusty));
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn hello() {
        let client = Client::tracked(super::launch()).expect("valid rocket instance");

        let res = client
            .post(uri!(super::call))
            .json(&rusty! {
                (hello: &str) -> String {
                    format!("{hello} world")
                } where { "Hi" }
            })
            .dispatch();

        assert_eq!(res.status(), Status::Ok);
        assert_eq!(res.into_json::<String>().unwrap(), "Hi world");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 12)]
    #[cfg_attr(miri, ignore)]
    async fn io_stream() {
        use rocket::local::asynchronous::Client;

        let client = Client::tracked(super::launch()).await.expect("valid rocket instance");
        let sleep_ms = |ms| time::sleep(Duration::from_millis(ms));

        let server = async {
            client
                .post(uri!(super::call))
                .json(&rusty! {
                    (hello: &str) {
                        eprintln!("{hello} world")
                    } where { "Hi" }
                })
                .dispatch()
                .await;
        };

        let listener = async {
            let bytes =
                client.get(uri!(super::stream)).dispatch().await.into_bytes().await.unwrap();
            assert_eq!(&bytes[..8], b"Hi world");
        };

        join!(
            async {
                sleep_ms(250).await; // time to establish `listener` connection 
                server.await;
                sleep_ms(250).await; // time to graceful shutdown

                client.rocket().shutdown().notify();
            },
            listener
        );
    }
}
