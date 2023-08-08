#![allow(clippy::let_unit_value)]
// false positive: https://github.com/SergioBenitez/Rocket/issues/2568

mod script;

use {
    chrono::Local,
    json::value::RawValue,
    moka::future::Cache,
    rocket::{
        figment::providers::Env,
        response::{content::RawJson, stream::ByteStream},
        serde::json::Json,
        Config, Shutdown, State,
    },
    std::{
        borrow, env, fmt, mem,
        sync::atomic::{AtomicUsize, Ordering},
    },
    tokio::{
        select,
        sync::broadcast::{channel, error::RecvError, Sender},
    },
};

#[derive(serde::Deserialize)]
struct Params<T> {
    params: T,
}

#[derive(serde::Deserialize)]
pub struct Call<'a> {
    jwt: Option<&'a str>,

    #[serde(borrow)]
    code: borrow::Cow<'a, str>,

    #[serde(borrow, default = "raw_null")]
    data: &'a RawValue,
}

fn raw_null() -> &'static RawValue {
    // Safety: `RawValue` is an `transparent` unsized newvalue above str
    unsafe { mem::transmute::<&str, _>("null") }
}

fn eat_str<T: ToString>(me: T) -> String {
    me.to_string()
}

// todo: possible to use in config, it is very easy:
//  - https://rocket.rs/v0.5-rc/guide/configuration/#configuration
//  - https://crates.io/keywords/configuration
const CRATES: &str = "crates";

#[post("/call", data = "<call>")]
async fn call(
    call: Json<Params<Call<'_>>>,
    scripts: &State<Scripts>,
    tx: &State<Sender<Vec<u8>>>,
) -> Result<RawJson<String>, script::Error> {
    fn unique_rs() -> String {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        format!("_{}", COUNT.fetch_add(1, Ordering::SeqCst))
    }

    let Params { params: call } = call.into_inner();

    let src = call.code.as_ref();
    let mut bytes = Vec::with_capacity(128);
    let file = scripts.cache.entry_by_ref(src).or_insert_with(async { unique_rs() }).await;

    #[cfg(feature = "pretty-trace")]
    match syn::parse_str::<syn::Expr>(src) {
        Ok(fmt) => {
            if cfg!(feature = "pretty-trace") {
                tracing::info!(
                    "Provided code:{}",
                    format!("\n{}", prettyplease::unparse_expr(&fmt)).replace('\n', "\n      ")
                );
            }
        }
        Err(err) => tracing::warn!("{err}"),
    }

    let out = script::execute_in(
        (&env::current_dir()?.join(CRATES), &file.into_value()),
        call,
        &mut bytes,
    )
    .await
    .map_err(eat_str);

    // A send 'fails' if there are no active subscribers. That's okay.
    let _ = tx.send(bytes);

    Ok(RawJson(match out {
        Ok(res) => res,
        Err(err) => format!("{{\"rejected\": {}}}", json::json!(err)),
    }))
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

use rocket::{get, post, routes};

#[rocket::launch]
fn rocket() -> _ {
    #[post("/init")]
    fn init() {}

    #[get("/healthz")]
    fn health() -> &'static str {
        "Service is up and running"
    }

    use tracing_subscriber::fmt::format::Writer;

    let timer: fn(&mut Writer<'_>) -> fmt::Result =
        |w| write!(w, "{}", Local::now().format("%Y-%m-%d %H:%M:%S"));

    if cfg!(not(test)) {
        tracing_subscriber::fmt().pretty().with_timer(timer).with_target(false).init();
    }

    let figment = if cfg!(docker_image) {
        Config::figment().merge(Env::raw().only(&["port"]))
    } else {
        Config::figment()
    };

    rocket::custom(figment)
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
        json::{json, Value},
        rocket::{form::validate::Contains, http::Status, local::blocking::Client, uri},
        std::time::Duration,
        tokio::{join, time},
    };

    macro_rules! rusty {
        (($($pats:tt)*) $(-> $ty:ty)? { $($body:tt)* } $(where $args:expr)? ) => {{
            // fixme: we should to do the IDE analyze this code - but not too much
            // fn __compile_check() {
            //      fn main($($pats)*) $(-> $ty)? { $($body)* }
            // }
            json::json!({
                "params": {
                    "code": stringify!(
                        async |$($pats)*| $(-> $ty)? { $($body)* }
                    ),
                    $("data": $args)?
                }
            })
        }};
    }

    fn rocket() -> Client {
        Client::tracked(super::rocket()).expect("valid rocket instance")
    }

    #[test]
    fn rusty() {
        fn clean(json: Value) -> String {
            json.to_string().replace(char::is_whitespace, "").replace("\\n", "")
        }

        let raw = json::json!({
            "params": {
                "code": r#"
                    async |hello: &str| -> String {
                        format!("{hello} world")
                    }"#,
                "data": "Hi"
            }
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
        let client = rocket();

        let res = client
            .post(uri!(super::call))
            .json(&rusty! {
                (Ctx { data: hello, .. }: Ctx<&str>) -> String {
                    format!("{hello} world")
                } where { "Hi" }
            })
            .dispatch();

        assert_eq!(res.status(), Status::Ok);
        assert_eq!(res.into_json::<Value>().unwrap(), json!({ "resolved": "Hi world" }))
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn forbid_stdout() {
        let client = rocket();

        let res = client
            .post(uri!(super::call))
            .json(&rusty! {
                (Ctx { data: (), .. }: Ctx<_>) {
                    println!("Hello, World!")
                }
            })
            .dispatch();

        assert_eq!(res.status(), Status::Ok);

        let text = res.into_string().unwrap();
        assert!(text[1..].starts_with("\"rejected\""));
        assert!(text.contains("print to `std{err, out}` forbidden"));
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn io_stream() {
        use rocket::local::asynchronous::Client;

        let client = Client::tracked(super::rocket()).await.expect("valid rocket instance");
        let sleep_ms = |ms| time::sleep(Duration::from_millis(ms));

        let server = async {
            client
                .post(uri!(super::call))
                .json(&rusty! {
                    (Ctx { data: hello, .. }: Ctx<&str>) {
                        #[wasm_bindgen]
                        extern "C" {
                            #[wasm_bindgen(js_namespace = console)]
                            fn error(s: &str);
                        }

                        error(&format!("{hello} world"));
                    } where { "Hi" }
                })
                .dispatch()
                .await;
        };

        let listener = async {
            let bytes =
                client.get(uri!(super::stream)).dispatch().await.into_bytes().await.unwrap();
            assert!(bytes.windows(8).any(|slice| slice == b"Hi world"));
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
