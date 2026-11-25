//! The books the server watches: each with a non-empty limit set, and each in force with a
//! close. At most [`MAX_WATCHED_BOOKS`] are; a first limit or a first close that would
//! watch one more is refused, read under the limit write lock so two first writes cannot
//! both take the last place. Retiring a closed book frees its place.

use std::collections::BTreeSet;

use axum::response::Response;
use convex_engine::ports::reference_master::*;
use convex_engine::ports::reference_repository::*;

use crate::book_close::close_of;
use crate::books::book_of;
use crate::limits::{conflict, set_of};

/// Books with a non-empty limit set, or in force with a close.
pub(crate) const MAX_WATCHED_BOOKS: usize = convex_engine::configuration::MAX_PLAN_BOOKS;

/// Whether `book` may be watched: it already is, or fewer than [`MAX_WATCHED_BOOKS`] other
/// books are.
pub(crate) async fn may_watch(
    repo: &dyn ReferenceRepository,
    book: &BookId,
) -> Result<bool, ReferenceError> {
    if is_watched(repo, book).await? {
        return Ok(true);
    }
    Ok(watched_books(repo).await?.len() < MAX_WATCHED_BOOKS)
}

/// Whether `book` is watched: it holds a non-empty limit set, or is in force with a close.
pub(crate) async fn is_watched(
    repo: &dyn ReferenceRepository,
    book: &BookId,
) -> Result<bool, ReferenceError> {
    if let Some(record) = repo.get_master(&MasterKey::LimitSet(book.clone())).await? {
        if !set_of(&record)?.limits.is_empty() {
            return Ok(true);
        }
    }
    let closed = repo
        .get_master(&MasterKey::BookClose(book.clone()))
        .await?
        .is_some();
    Ok(closed && is_in_force(repo, book).await?)
}

async fn is_in_force(
    repo: &dyn ReferenceRepository,
    book: &BookId,
) -> Result<bool, ReferenceError> {
    Ok(repo
        .get_master(&MasterKey::Book(book.clone()))
        .await?
        .as_ref()
        .map(book_of)
        .transpose()?
        .is_some_and(|book| !book.is_retired()))
}

/// Every watched book.
pub(crate) async fn watched_books(
    repo: &dyn ReferenceRepository,
) -> Result<BTreeSet<BookId>, ReferenceError> {
    let mut watched: BTreeSet<BookId> = every_master(repo, MasterKind::LimitSet, |record| {
        let set = set_of(record)?;
        Ok((!set.limits.is_empty()).then(|| set.book_id.clone()))
    })
    .await?
    .into_iter()
    .collect();
    let closed = every_master(repo, MasterKind::BookClose, |record| {
        Ok(Some(close_of(record)?.book_id.clone()))
    })
    .await?;
    for book in closed {
        if !watched.contains(&book) && is_in_force(repo, &book).await? {
            watched.insert(book);
        }
    }
    Ok(watched)
}

/// What `pick` takes from each current record of `kind`, page by page.
pub(crate) async fn every_master<T>(
    repo: &dyn ReferenceRepository,
    kind: MasterKind,
    mut pick: impl FnMut(&MasterRecord) -> Result<Option<T>, ReferenceError>,
) -> Result<Vec<T>, ReferenceError> {
    let mut picked = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = repo
            .list_masters(kind, after.as_deref(), MAX_REFERENCE_PAGE)
            .await?;
        for record in &page {
            picked.extend(pick(record)?);
        }
        if page.len() < MAX_REFERENCE_PAGE {
            return Ok(picked);
        }
        let cursor = page.last().and_then(crate::reference::page_cursor);
        after = Some(cursor.ok_or(ReferenceError::Unsupported(
            "a record without a page cursor",
        ))?);
    }
}

pub(crate) fn too_many_watched() -> Response {
    conflict(
        "WATCHED_BOOKS_EXCEEDED",
        &format!(
            "at most {MAX_WATCHED_BOOKS} books are watched: those with a non-empty limit set, or in force with a close"
        ),
    )
}
