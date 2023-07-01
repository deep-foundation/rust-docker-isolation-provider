#![allow(clippy::let_unit_value)] // false positive: https://github.com/SergioBenitez/Rocket/issues/2568

mod script;

use {
    json::value::RawValue,
    moka::future::Cache,
    rocket::{response::content::RawJson, serde::json::Json, State},
    std::{
        borrow, env,
        sync::atomic::{AtomicUsize, Ordering},
    },
};

use rocket::{get, post, routes};

#[derive(serde::Deserialize)]
pub struct Call<'a> {
    #[serde(default, borrow)]
    head: borrow::Cow<'a, str>,

    #[serde(borrow)]
    main: borrow::Cow<'a, str>,

    #[serde(borrow)]
    args: &'a RawValue,
}

// fixme: possible to use in config, it is very easy - https://crates.io/keywords/configuration
const CRATES: &str = "crates";

#[post("/call", data = "<call>")]
async fn call(
    call: Json<Call<'_>>,
    scripts: &State<Scripts>,
) -> Result<RawJson<String>, script::Error> {
    static COUNT: AtomicUsize = AtomicUsize::new(0);

    async fn unique_rs() -> String {
        format!("{}.rs", COUNT.fetch_add(1, Ordering::SeqCst))
    }

    let file = scripts.cache.entry_by_ref(call.main.as_ref()).or_insert_with(unique_rs()).await;
    script::execute_in(
        (&env::current_dir()?.join(CRATES), &file.into_value()),
        call.into_inner(), // keep formatting
    )
    .await
    .map(RawJson)
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
        .manage(Scripts { cache: Cache::new(8096) })
        .mount("/", routes![init, health, call])
}
