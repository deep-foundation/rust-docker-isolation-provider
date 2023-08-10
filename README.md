Any handler is an asynchronous closure of the following signature `Fn(Ctx<impl Deserialize>) -> Serialize`.\
Where [`Ctx`](https://github.com/deep-foundation/rust-docker-isolation-provider/blob/main/template/src/lib.trs#L171-L174)
is:

```rust
struct Ctx<T> {
    pub data: T,
    // impl Deserialize
    pub deep: Option<JsValue>, // null if `jwt` not provided
}
```

```rust
use serde_json as json;

async |Ctx { .. }: Ctx<json::Value>| {
    2 + 2
}
```

By default, the handler has `serde` and `serde_json` in its dependencies. They can be overridden, or new ones can be
added using the syntax:

```rust
where cargo: {
    [dependencies]
    chrono = { version = "0.4" }
    serde_json = { features = ["preserve_order"] }
}

// it eat any provided json 
async |_: Ctx| -> String {
    chrono::Local::now().to_string()
}
```

This directly [merges](https://github.com/deep-foundation/rust-docker-isolation-provider/blob/main/src/script.rs#L115)
it with the `Cargo.toml` associated with this handler.
