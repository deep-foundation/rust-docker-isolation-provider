#![allow(clippy::let_unit_value)] // false positive: https://github.com/SergioBenitez/Rocket/issues/2568
#![feature(async_closure)]

mod script;

use {
    rocket::{response::content::RawJson, serde::json::Json},
    std::{
        env,
        sync::atomic::{AtomicUsize, Ordering},
    },
};

use rocket::{get, post, routes};

#[derive(serde::Deserialize)]
struct Call<'a> {
    code: &'a str,
    args: json::Value,
}

// fixme: possible to use in config, it is very easy - https://crates.io/keywords/configuration
const CRATES: &str = "crates";

#[post("/call", data = "<call>")]
async fn call(call: Json<Call<'_>>) -> Result<RawJson<String>, script::Error> {
    static COUNT: AtomicUsize = AtomicUsize::new(0);

    fn unique_rs() -> String {
        format!("{}.rs", COUNT.fetch_add(1, Ordering::SeqCst))
    }

    let repr = call.args.to_string();
    script::execute_in(
        (&env::current_dir()?.join(CRATES), &unique_rs()),
        (call.code, &repr), // keep formatting
    )
    .await
    .map(RawJson)
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
        // .manage(...)
        .mount("/", routes![init, health, call])
}
