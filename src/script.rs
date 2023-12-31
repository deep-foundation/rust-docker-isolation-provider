use {
    crate::Call,
    fs_extra::dir::CopyOptions,
    rocket::{
        http::Status,
        response::{self, Responder},
        Request,
    },
    std::{env, fmt, fs, path::Path, time::Instant},
    toml::Table,
    tracing::info,
};

#[derive(Debug)]
pub enum Error {
    Internal(Box<dyn std::error::Error + Sync + Send>),
    Compiler(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Internal(err) => write!(f, "{err}"),
            Error::Compiler(err) => write!(f, "{err}"),
        }
    }
}

impl<'r, 'o: 'r> Responder<'r, 'o> for Error {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'o> {
        match self {
            Self::Internal(err) => {
                tracing::error!("Bug: {err}");
                Err(Status::InternalServerError)
            }
            Self::Compiler(err) => (Status::UnprocessableEntity, err).respond_to(request),
        }
    }
}

impl<E: Into<Box<dyn std::error::Error + Sync + Send + 'static>>> From<E> for Error {
    fn from(error: E) -> Self {
        Self::Internal(error.into())
    }
}

const MOD_JS: &str = include_str!("../template/mod.tmjs");
const CARGO: &str = include_str!("../template/Cargo.ttoml"); // fixme: better use once static table
const TEMPLATE: &str = include_str!("../template/src/lib.trs");

// todo: try to replace `(from, to)` into hashmap
//  and use regex
pub fn expand(src: &str, [from, to]: [&str; 2]) -> String {
    src.replace(from, to)
}

// fixme: use non-blocking `fs` from `tokio`
#[tracing::instrument(skip(path, stderr, src))]
pub async fn execute_in(
    (path, id): (&Path, usize),
    Call { jwt, code: (manifest, src), data }: Call<'_>,
    stderr: &mut Vec<u8>,
) -> Result<String, Error> {
    let file = format!("_{id}");
    let dir = path.join(&file);

    let _ = fs::create_dir(&dir);

    fs_extra::dir::copy(env::current_dir()?.join("template"), &dir, &options())?;

    let dir = dir.join("template");
    fs::write(dir.join("mod.mjs"), expand(MOD_JS, ["#{name}", &file]))?;
    fs::write(dir.join("src/lib.rs"), expand(TEMPLATE, ["#{main}", &src]))?;

    let cargo = expand(CARGO, ["#{name}", &format!("\"{file}\"")]);
    fs::write(
        dir.join("Cargo.toml"),
        if let Some(manifest) = manifest {
            merge_manifest(cargo.parse()?, manifest)?
        } else {
            cargo
        },
    )?;

    macro_rules! troo {
        ($exec:expr => $($args:expr)*) => {{
            let instant = Instant::now();
            let out = tokio::process::Command::new($exec)
                $(.arg(AsRef::<std::ffi::OsStr>::as_ref(&$args)))* .output().await?;
            if out.status.success() {
                stderr.extend(out.stderr);
                (out.stdout, instant.elapsed())
            } else {
                let err = String::from_utf8(out.stderr)?;
                tracing::error!("{err}");
                return Err(Error::Compiler(err));
            }
        }};
    }

    let (_, elapsed) = troo! { "wasm-pack" => "build" "--target" "nodejs" dir };
    info!("Compilation time: {elapsed:?}");

    // fixme: maybe install one time in Docker image?
    // let _ = troo! {
    //     if cfg!(target_os = "windows") { "npm.cmd" } else { "npm" }
    //         => "install" "-g" "@deep-foundation/deeplinks"
    // };

    let (out, elapsed) = troo! {
        "node" => dir.join("mod.mjs") data.get() jwt.unwrap_or("")
    };
    info!("Execution time: {elapsed:?}");

    Ok(String::from_utf8(out)?)
}

fn options() -> CopyOptions {
    CopyOptions { skip_exist: true, copy_inside: true, ..CopyOptions::default() }
}

fn merge_manifest(mut into: Table, from: Table) -> Result<String, Error> {
    use toml::{map::Entry, Value};

    for (key, val) in from {
        match val {
            Value::Table(from) => match into.entry(key) {
                Entry::Vacant(e) => {
                    e.insert(Value::Table(from));
                }
                Entry::Occupied(e) => {
                    e.into_mut()
                        .as_table_mut()
                        .ok_or("cannot merge manifests: cannot merge table and non-table values")?
                        .extend(from);
                }
            },
            other => {
                into.insert(key, other);
            }
        }
    }

    Ok(format!("{into}"))
}

#[test]
fn merge_toml() {
    use toml::toml;

    let first = toml! {
        [dependencies]
        a = "first"

        [profile.foo]
        incremental = false
    };
    let second = toml! {
        [dependencies]
        b = "second"

        [profile.foo]
        incremental = true
    };
    assert_eq!(
        merge_manifest(first, second).unwrap(),
        toml! {
            [dependencies]
            a = "first"
            b = "second"

            [profile.foo]
            incremental = true
        }
        .to_string()
    );
}
