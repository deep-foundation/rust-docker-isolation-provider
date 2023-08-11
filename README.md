# Rust provider

Any handler is an asynchronous closure of the following signature `Fn(Ctx<impl Deserialize>) -> Serialize`.\
Where [`Ctx`](https://github.com/deep-foundation/rust-docker-isolation-provider/blob/main/template/src/lib.trs#L171-L174)
is:

```rust
struct Ctx<T> {
    pub data: T, // impl `Deserialize`
    pub deep: Option<JsValue>, // `None` if `jwt` not provided
}
```

```rust
use serde_json as json;

// Hello world handler in general form
async |Ctx { _data, _deep, .. }: Ctx<json::Value>| {
    2 + 2
}
```
#### Other handlers signatures 
```rust
async |_: Ctx<T>| -> U {} // full typed 
async |_: Ctx| {} // dynamic json + type inference 
async |_| {} // short 
```

### [`js!`](https://github.com/deep-foundation/rust-docker-isolation-provider/blob/main/template/embed/src/lib.rs#L34-L73) macro
It allow to execute any JS snippet like this (now only in the async form):
```rust
async |Ctx { data: (a, b), .. }: Ctx<(i32, i32)>| -> i32 {
    // macro depends on the type inference
    // so we just have to provide type               ^^^
    js!(|a, b| { // `a` and `b` are implicit captures
        console.error(a, b);
        return a + b;
    })
    .await 
}
```

### Manifest forwarding

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
