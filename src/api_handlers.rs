use warp::{Filter};
use serde::Deserialize;
use std::process::Command;
use std::fs::File;
use std::io::Write;

#[derive(Deserialize)]
pub struct CodeExecutionRequest {
    code: String,
    args: Vec<String>,
}

pub async fn execute_code(request: CodeExecutionRequest) -> Result<impl warp::Reply, warp::Rejection> {
    // Записываем код в файл
// Записываем аргументы в файл
    let mut args_file = match File::create("args.txt") {
        Ok(file) => file,
        Err(e) => return Ok(warp::reply::with_status(format!("{}", e), warp::http::StatusCode::INTERNAL_SERVER_ERROR)),
    };
    if let Err(e) = args_file.write_all(request.args.join("\n").as_bytes()) {
        return Ok(warp::reply::with_status(format!("{}", e), warp::http::StatusCode::INTERNAL_SERVER_ERROR));
    }

// Компилируем код с помощью `rustc`
    let output = match Command::new("rustc")
        .arg("exec_code.rs")
        .output() {
        Ok(output) => output,
        Err(e) => return Ok(warp::reply::with_status(format!("{}", e), warp::http::StatusCode::INTERNAL_SERVER_ERROR)),
    };

// Если при компиляции возникли ошибки, возвращаем их
    if !output.stderr.is_empty() {
        return Ok(warp::reply::with_status(format!("Compilation error: {}", String::from_utf8_lossy(&output.stderr)), warp::http::StatusCode::INTERNAL_SERVER_ERROR));
    }

// Запускаем скомпилированный код
    let output = match Command::new("./exec_code")
        .args(&request.args)  // передаем аргументы
        .output() {
        Ok(output) => output,
        Err(e) => return Ok(warp::reply::with_status(format!("{}", e), warp::http::StatusCode::INTERNAL_SERVER_ERROR)),
    };


    // Возвращаем результат выполнения кода
    Ok(warp::reply::with_status(String::from_utf8_lossy(&output.stdout).into_owned(), warp::http::StatusCode::OK))
}
pub async fn healthz() -> Result<impl warp::Reply, warp::Rejection> {
    Ok(warp::reply::with_status(
        "Service is up and running",
        warp::http::StatusCode::OK,
    ))
}


pub fn call_route() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    let execute_code_route = warp::path!("call")
        .and(warp::post())
        .and(warp::body::json::<CodeExecutionRequest>())
        .and_then(execute_code);

    let healthz_route = warp::path!("healthz")
        .and(warp::get())
        .and_then(healthz);

    execute_code_route.or(healthz_route)
}