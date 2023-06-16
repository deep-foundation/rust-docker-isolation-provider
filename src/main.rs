mod api_handlers;
// mod functions; // раскомментируйте эту строку, когда у вас появятся функции для импорта

use api_handlers::{call_route};
// use functions::my_function; // раскомментируйте эту строку, когда у вас появится функция my_function

#[tokio::main]
async fn main() {
    let routes =call_route();

    warp::serve(routes).run(([127, 0, 0, 1], 8000)).await;
}