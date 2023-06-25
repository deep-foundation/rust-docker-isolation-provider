use {
    std::path::Path,
    tokio::{fs, process::Command},
};

use rocket::{
    http::Status,
    response::{self, Debug, Responder},
    Request,
};

pub enum Error {
    Internal(Box<dyn std::error::Error + Sync + Send>), // to avoid anyhow as dependency
    Compile(String),
}

impl<'r, 'o: 'r> Responder<'r, 'o> for Error {
    fn respond_to(self, request: &'r Request<'_>) -> response::Result<'o> {
        match self {
            Self::Internal(err) => Debug(err).respond_to(request),
            Self::Compile(err) => (Status::UnprocessableEntity, err).respond_to(request),
        }
    }
}

impl<E: std::error::Error + Sync + Send + 'static> From<E> for Error {
    fn from(error: E) -> Self {
        Self::Internal(error.into())
    }
}

pub async fn execute_in(path: &Path, code: &str, raw: &str) -> Result<String, Error> {
    let _ = fs::create_dir("crates").await;

    fs::write(
        path,
        format!(
            // todo: later try to use templates
            "fn main() -> Result<(), Box<dyn std::error::Error>> {{ \
                let args = serde_json::from_str(r#\"{raw}\"#)?; \
                {code} println!(\"{{}}\", serde_json::to_string(&main(args))?); Ok(()) }}"
        ),
    )
    .await?;

    let out = Command::new("rust-script").arg("-d serde_json").arg(path).output().await?;

    if out.status.success() {
        Ok(String::from_utf8(out.stdout)?)
    } else {
        Err(Error::Compile(String::from_utf8(out.stderr)?))
    }
}
