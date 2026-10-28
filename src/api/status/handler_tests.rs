use axum::{
    extract::State,
    http::header,
    response::IntoResponse,
};

use super::handle_status;
use crate::{
    AppState,
    api::shared::cache::NO_STORE_CACHE_CONTROL,
    init_db,
};

#[tokio::test]
async fn test_status_endpoint() {
    let db_conn = init_db("sqlite::memory:").await.unwrap();
    let state = AppState::new(db_conn);
    let response = handle_status(State(state)).await.into_response();

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        NO_STORE_CACHE_CONTROL
    );
}
