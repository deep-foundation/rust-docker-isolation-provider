#![allow(clippy::let_unit_value)] // false pos.: https://github.com/SergioBenitez/Rocket/issues/2568

mod parse;
mod script;

use {
    chrono::Local,
    json::value::RawValue,
    moka::future::Cache,
    rocket::{
        figment::providers::Env, response::content::RawJson, serde::json::Json, Config, State,
    },
    serde::{de::Error, Deserialize, Deserializer},
    std::{
        borrow::Cow,
        env, fmt, fs, io, mem,
        sync::atomic::{AtomicUsize, Ordering},
    },
    tokio::{
        self,
        sync::{mpsc, oneshot},
    },
};

#[cfg(feature = "bytes-stream")]
use {
    rocket::{response::stream::ByteStream, Shutdown},
    tokio::sync::broadcast::{self, error::RecvError},
};

#[cfg(feature = "pretty-trace")]
use tracing::{info, warn};

#[derive(serde::Deserialize)]
struct Params<T> {
    params: T,
}

#[derive(serde::Deserialize, Debug)]
pub struct Call<'a> {
    jwt: Option<&'a str>,

    #[serde(deserialize_with = "manifesty")]
    code: (Option<toml::Table>, Cow<'a, str>),

    #[serde(borrow, default = "raw_null")]
    data: &'a RawValue,
}

fn manifesty<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<(Option<toml::Table>, Cow<'de, str>), D::Error> {
    use Cow::{Borrowed, Owned};

    match Cow::deserialize(deserializer)? {
        Borrowed(src) => parse::extract_manifest(src).map(|(a, b)| (a, Borrowed(b))),
        Owned(ref str) => parse::extract_manifest(str).map(|(a, b)| (a, Owned(b.to_owned()))),
    }
    .map_err(Error::custom)
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

struct Context {
    #[cfg(feature = "bytes-stream")]
    bytes: broadcast::Sender<Vec<u8>>,
    scripts: Cache<String, usize>,
    compile: mpsc::Sender<Input>,
}

type Input = (usize, Call<'static>, oneshot::Sender<Output>);
type Output = (Result<String, script::Error>, Vec<u8>);

#[post("/call", data = "<call>")]
async fn call(
    call: Json<Params<Call<'_>>>,
    ctx: &State<Context>,
) -> Result<RawJson<String>, script::Error> {
    static COUNT: AtomicUsize = AtomicUsize::new(0);

    fn unique() -> usize {
        COUNT.fetch_add(1, Ordering::SeqCst)
    }

    let Params { params: call } = call.into_inner();
    let Context { scripts, compile, .. } = ctx.inner();

    let src = call.code.1.as_ref();
    let file = scripts.entry_by_ref(src).or_insert_with(async { unique() }).await;

    #[cfg(feature = "pretty-trace")]
    match syn::parse_str::<syn::Expr>(src) {
        Ok(fmt) => {
            let tab = "\n      ";
            if cfg!(feature = "pretty-trace") {
                info!("Provided code:{tab}{}", prettyplease::unparse_expr(&fmt).replace('\n', tab));
            }
        }
        Err(err) => warn!("Possible error syntax: {err}"),
    }

    let (tx, rx) = oneshot::channel();
    let call: Call<'static> = unsafe {
        // Safety: We recv result back earlier than ref supposedly dies
        mem::transmute(call)
    };
    let _ = compile.send((file.into_value(), call, tx)).await;

    #[allow(unused_variables)]
    let (out, bytes) = rx.await.unwrap(/* invalid sender usage */);

    tracing::debug!(bytes = String::from_utf8_lossy(&bytes).as_ref());

    // A send 'fails' if there are no active subscribers. That's okay.
    #[cfg(feature = "bytes-stream")]
    let _ = ctx.bytes.send(bytes);

    Ok(RawJson(match out.map_err(eat_str) {
        Ok(res) => res,
        Err(err) => format!("{{\"rejected\": {}}}", json::json!(err)),
    }))
}

#[cfg(feature = "bytes-stream")]
#[get("/stream")]
fn stream(ctx: &State<Context>, mut end: Shutdown) -> ByteStream![Vec<u8>] {
    let mut rx = ctx.bytes.subscribe();

    ByteStream! {
        loop {
            yield tokio::select! {
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

use rocket::{get, post, routes};

#[rocket::launch]
async fn rocket() -> _ {
    #[post("/init")]
    fn init() {}

    #[get("/healthz")]
    fn health() -> &'static str {
        "Service is up and running"
    }

    use tracing_subscriber::{fmt::format::Writer, EnvFilter};

    let timer: fn(&mut Writer<'_>) -> fmt::Result =
        |w| write!(w, "{}", Local::now().format("%Y-%m-%d %H:%M:%S"));

    if cfg!(not(test)) {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .with_timer(timer)
            .with_target(false)
            .pretty()
            .init();
    }

    let figment = if cfg!(docker_image) {
        Config::figment().merge(Env::raw().only(&["port"]))
    } else {
        Config::figment()
    };

    let (tx, mut rx) = mpsc::channel::<Input>(32);
    tokio::spawn(async move {
        let _loop = tracing::debug_span!("compiler_loop");

        let path = env::current_dir()?.join(CRATES);
        tracing::debug!(?path);

        let _ = fs::create_dir(&path);
        fs::write(path.join("Cargo.toml"), include_str!("../template/Workspace.toml"))?;

        while let Some((id, call, ret)) = rx.recv().await {
            let mut bytes = Vec::with_capacity(128);
            let _ = ret.send((script::execute_in((&path, id), call, &mut bytes).await, bytes));
        }
        io::Result::Ok(())
    });

    #[allow(unused_mut)]
    let mut routes = routes![];

    #[cfg(feature = "bytes-stream")]
    {
        routes.extend(routes![stream]);
    }

    rocket::custom(figment)
        .manage(Context {
            #[cfg(feature = "bytes-stream")]
            bytes: broadcast::channel::<Vec<u8>>(1024).0,
            scripts: Cache::new(8096),
            compile: tx,
        })
        .mount("/", routes![init, health, call])
        .mount("/", routes)
}

// todo: extract into `tests/` folder
//  it may seem excessive because you need to create `lib.rs` as well,
//  which will lead to a loss of minimalism
#[cfg(test)]
mod tests {
    use {
        json::{json, Value},
        rocket::{form::validate::Contains, http::Status, local::asynchronous::Client, uri},
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

    async fn rocket() -> Client {
        Client::tracked(super::rocket().await).await.expect("valid rocket instance")
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

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn hello() {
        let client = rocket().await;

        let res = client
            .post(uri!(super::call))
            .json(&rusty! {
                (Ctx { data: hello, .. }: Ctx<&str>) -> String {
                    format!("{hello} world")
                } where { "Hi" }
            })
            .dispatch()
            .await;

        assert_eq!(res.status(), Status::Ok);
        assert_eq!(res.into_json::<Value>().await.unwrap(), json!({ "resolved": "Hi world" }))
    }

    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn forbid_stdout() {
        let client = rocket().await;

        let res = client
            .post(uri!(super::call))
            .json(&rusty! {
                (_nothing) {
                    println!("Hello, World!")
                }
            })
            .dispatch()
            .await;

        assert_eq!(res.status(), Status::Ok);

        let text = res.into_string().await.unwrap();
        assert!(text[1..].starts_with("\"rejected\""));
        assert!(text.contains("print to `std{err, out}` forbidden"));
    }

    #[cfg(feature = "bytes-stream")]
    #[tokio::test]
    #[cfg_attr(miri, ignore)]
    async fn io_stream() {
        use {
            std::time::Duration,
            tokio::{join, time},
        };

        let client = Client::tracked(super::rocket().await).await.expect("valid rocket instance");
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
