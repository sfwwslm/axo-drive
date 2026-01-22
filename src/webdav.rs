//! WebDAV request handler wrapper.

use axum::extract::Extension;
use axum::http::Request;
use axum::response::Response;
use dav_server::{DavHandler, body::Body as DavBody};
use std::sync::Arc;

pub async fn webdav_handler(
    Extension(dav_handler): Extension<Arc<DavHandler>>,
    req: Request<axum::body::Body>,
) -> Response<DavBody> {
    dav_handler.handle(req).await
}
