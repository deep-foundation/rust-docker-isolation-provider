use {
    fs_extra::dir::CopyOptions,
    std::{env, fmt, fs, path::Path, time::Instant},
    tracing::info,
};

use {
    crate::Call,
    rocket::{
        http::Status,
        response::{self, Debug, Responder},
        Request,
    },
};

pub enum Error {
    Internal(Box<dyn std::error::Error + Sync + Send>), // to avoid `anyhow` as dependency
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
            Self::Internal(err) => Debug(err).respond_to(request),
            Self::Compiler(err) => (Status::UnprocessableEntity, err).respond_to(request),
        }
    }
}

impl<E: std::error::Error + Sync + Send + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self::Internal(error.into())
    }
}

const TEMPLATE: &str = include_str!("../template/src/lib.trs");

// todo: try to replace `(from, to)` into hashmap
//  and use regex
pub fn expand(src: &str, [from, to]: [&str; 2]) -> String {
    src.replace(from, to)
}

#[tracing::instrument(skip(path, stderr, code))]
pub async fn execute_in(
    (path, file): (&Path, &str),
    Call { jwt, code, data }: Call<'_>,
    stderr: &mut Vec<u8>,
) -> Result<String, Error> {
    let dir = path.join(file);

    let _ = fs::create_dir(path);
    let _ = fs::create_dir(&dir);

    fs_extra::dir::copy(env::current_dir().unwrap().join("template"), &dir, &options()).unwrap();

    let dir = dir.join("template");
    fs::write(dir.join("src/lib.rs"), expand(TEMPLATE, ["#{main}", &code])).unwrap();

    macro_rules! troo {
        ($exec:expr => $($args:expr)*) => {{
            let instant = Instant::now();
            let out = tokio::process::Command::new($exec)
                $(.arg(AsRef::<std::ffi::OsStr>::as_ref(&$args)))* .output().await.unwrap();
            if out.status.success() {
                stderr.extend(out.stderr);
                (out.stdout, instant.elapsed())
            } else {
                let err = String::from_utf8(out.stderr).unwrap();
                tracing::error!("{err}");
                return Err(Error::Compiler(err));
            }
        }};
    }

    let (_, elapsed) = troo! { "wasm-pack" => "build" "--target" "nodejs" "--dev" dir };
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

    Ok(String::from_utf8(out).unwrap())
}

fn options() -> CopyOptions {
    CopyOptions { skip_exist: true, copy_inside: true, ..CopyOptions::default() }
}
