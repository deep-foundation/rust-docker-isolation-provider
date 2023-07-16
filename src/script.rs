use {
    std::path::Path,
    tokio::{fs, process},
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

const TEMPLATE: &str = include_str!("template.trs");

// todo: try to replace `(from, to)` into hashmap
//  and use regex
pub fn expand(src: &str, [from, to]: [&str; 2]) -> String {
    src.replace(from, to)
}

pub async fn execute_in(
    (path, file): (&Path, &str),
    Call { head: _head, code, data }: Call<'_>,
) -> Result<String, Error> {
    let _ = fs::create_dir(path).await;

    fs::write(path.join(file), expand(TEMPLATE, ["#{main}", &code])).await?;

    let out = process::Command::new("rust-script")
        .args(["--toolchain", "nightly"])
        .arg(path.join(file))
        .arg(data.get())
        .output()
        .await?;

    if out.status.success() {
        Ok(String::from_utf8(out.stdout)?)
    } else {
        Err(Error::Compiler(String::from_utf8(out.stderr)?))
    }
}
