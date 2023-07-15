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
    #[serde(borrow, default)]
    head: borrow::Cow<'a, str>,

    #[serde(borrow)]
    code: borrow::Cow<'a, str>,

    #[serde(borrow)]
    data: &'a RawValue,
}

// todo: possible to use in config, it is very easy:
//  - https://rocket.rs/v0.5-rc/guide/configuration/#configuration
//  - https://crates.io/keywords/configuration
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

    let file = scripts.cache.entry_by_ref(call.code.as_ref()).or_insert_with(unique_rs()).await;
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

// todo: extract into `tests/` folder
//  it may seem excessive because you need to create `lib.rs` as well,
//  which will lead to a loss of minimalism
#[cfg(test)]
mod tests {
    use {
        json::{json, Value},
        rocket::{local::blocking::Client, uri},
    };

    #[test]
    #[cfg_attr(miri, ignore)]
    fn hello() {
        let client = Client::tracked(super::launch()).expect("valid rocket instance");

        let res = client
            .post(uri!(super::call))
            .json(&json!({
                "code": r#"fn main(hello: &str) -> String {
                    format!("{hello} world")
                }"#,
                "data": "Hi"
            }))
            .dispatch();

        assert_eq!(res.into_json::<Value>().unwrap(), json!({ "resolved": "Hi world" }));
    }
}
