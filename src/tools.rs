//! The two tools as plain async functions over a `Kavita`, the counters, the handles and the caller's
//! identity. The rmcp layer (`http.rs`) only extracts the identity and the elicitation; the
//! confirmation is passed in as `ask`, so every path that must send nothing is testable without a client.
//!
//! `Ok` is text for the model; `Err` is text too, returned as a tool error.

use std::future::Future;
use std::time::Instant;

use rmcp::schemars;
use serde::Deserialize;

use crate::config::Identity;
use crate::handles::{Book, Handles};
use crate::kavita::{Chapter, Format, Kavita, KavitaError, SeriesHit, KINDLE};
use crate::messages::{self, clean};
use crate::metrics::{Metrics, KAVITA_FAILURES, REQUEST_FAILURES, SEARCH_SKIPPED, SEND_SKIPPED};

const MAX_TITLE: usize = 120;
const MAX_NAME: usize = 64;
const MAX_QUERY: usize = 100;
const TOP_SERIES: usize = 5;
const MAX_BOOKS: usize = 10;

/// Counts the fault and words it for the person.
pub(crate) fn fault(metrics: &Metrics, who: &Identity, e: &KavitaError) -> String {
    metrics.inc(KAVITA_FAILURES, e.kind());
    tracing::warn!(profile = %who.profile, kind = e.kind(), detail = ?e, "Kavita call failed");
    messages::kavita_fault(who.language, who.admin.as_deref(), e)
}

/// One sendable unit per chapter. Several files: one format only if all agree, and their sizes add up,
/// as Kavita's own send does.
#[must_use]
pub fn book_of(series: &SeriesHit, ch: &Chapter) -> Book {
    let first = ch.files.first().map_or(Format::Unknown, |f| f.format);
    let format = if ch.files.iter().all(|f| f.format == first) {
        first
    } else {
        Format::Unknown
    };
    let name = clean(&series.name, MAX_TITLE);
    let title = match clean(&ch.title_name, MAX_TITLE) {
        t if !t.is_empty() => t,
        _ if ch.range.is_empty() || ch.range == "1" => name,
        _ => format!("{name} {}", clean(&ch.range, 16)),
    };
    Book {
        chapter_id: ch.id,
        title,
        format,
        bytes: ch.files.iter().map(|f| f.bytes).sum(),
        library: clean(&series.library_name, MAX_NAME),
    }
}

/// # Errors
/// A Kavita fault, counted, as the person's sentence.
pub async fn search_library(
    kavita: &Kavita,
    metrics: &Metrics,
    handles: &Handles,
    who: &Identity,
    query: &str,
    limit: u64,
) -> Result<String, String> {
    let query = clean(query, MAX_QUERY);
    let hits = kavita
        .search(&who.kavita_key, &query)
        .await
        .map_err(|e| fault(metrics, who, &e))?;
    // Kavita already scopes search to the person's libraries; the configured grant is the second check,
    // and a book only becomes sendable by surviving it (a handle is minted below, nowhere else).
    let granted: Vec<SeriesHit> = hits.into_iter().filter(|s| {
        let ok = who.libraries.iter().any(|l| l == &s.library_name);
        if !ok {
            metrics.inc(SEARCH_SKIPPED, "library_not_granted");
            tracing::warn!(profile = %who.profile, library = %clean(&s.library_name, MAX_NAME),
                           "search hit outside the configured grant: Kavita and the configuration disagree");
        }
        ok
    }).take(TOP_SERIES).collect();

    let now = Instant::now();
    let mut lines = Vec::new();
    'series: for s in &granted {
        for ch in kavita
            .chapters(&who.kavita_key, s.series_id)
            .await
            .map_err(|e| fault(metrics, who, &e))?
        {
            if lines.len() == MAX_BOOKS {
                break 'series;
            }
            let b = book_of(s, &ch);
            let line = format!(
                "| « {} » | series: {} | library: {} | {} | {} | kindle: {}",
                b.title,
                clean(&s.name, MAX_TITLE),
                b.library,
                b.format.label(),
                messages::size(who.language, b.bytes),
                if b.format.kindle_ok() && b.bytes < limit {
                    "yes"
                } else {
                    "no"
                },
            );
            lines.push(format!(
                "handle={} {line}",
                handles.mint(&who.profile, b, now)
            ));
        }
    }
    if lines.is_empty() {
        return Ok(messages::nothing_found(who.language, &query));
    }
    Ok(lines.join("\n"))
}

/// The person's answer to the confirmation. A dismissed form is `Declined`; a timeout or a broken
/// exchange is `NoAnswer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Accepted,
    Declined,
    NoAnswer,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SendArgs {
    /// The handle search_library gave for the book. Never a number.
    pub handle: String,
    /// The Kindle's name, only when the person has several.
    #[serde(default)]
    pub kindle: Option<String>,
}

fn skipped(metrics: &Metrics, kind: &str, text: String) -> Result<String, String> {
    metrics.inc(SEND_SKIPPED, kind);
    Ok(text)
}

/// Checks run in this order and every refusal before the final send sends nothing: elicitation
/// support, the handle, the Kindle, the format, the size, Kavita's email setting, the confirmation.
///
/// # Errors
/// A Kavita fault, or email switched off, counted, as the person's sentence.
#[allow(clippy::too_many_arguments)]
pub async fn send_to_kindle<F, Fut>(
    kavita: &Kavita,
    metrics: &Metrics,
    handles: &Handles,
    who: &Identity,
    args: SendArgs,
    limit: u64,
    can_ask: bool,
    ask: F,
    cancelled: impl Fn() -> bool,
) -> Result<String, String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Answer>,
{
    let (l, admin) = (who.language, who.admin.as_deref());
    if !can_ask {
        metrics.inc(REQUEST_FAILURES, "elicitation_unsupported");
        return Ok(messages::no_elicitation(l));
    }
    let Some(book) = handles.resolve(&who.profile, args.handle.trim(), Instant::now()) else {
        return skipped(metrics, "unknown_handle", messages::unknown_handle(l));
    };
    let key = &who.kavita_key;
    let kindles: Vec<_> = kavita
        .devices(key)
        .await
        .map_err(|e| fault(metrics, who, &e))?
        .into_iter()
        .filter(|d| d.platform == KINDLE)
        .collect();
    let names: Vec<String> = kindles.iter().map(|d| clean(&d.name, MAX_NAME)).collect();
    let device = match (kindles.len(), args.kindle.as_deref()) {
        (0, _) => return skipped(metrics, "no_kindle", messages::no_kindle(l, admin)),
        (1, _) => &kindles[0],
        (_, Some(want)) => match kindles
            .iter()
            .zip(&names)
            .find(|(_, n)| n.eq_ignore_ascii_case(want.trim()))
        {
            Some((d, _)) => d,
            None => return Ok(messages::which_kindle(l, &names)),
        },
        (_, None) => return Ok(messages::which_kindle(l, &names)),
    };
    let device_name = clean(&device.name, MAX_NAME);
    if !book.format.kindle_ok() {
        return skipped(
            metrics,
            "format",
            messages::bad_format(l, &book.title, book.format.label()),
        );
    }
    if book.bytes >= limit {
        return skipped(
            metrics,
            "too_large",
            messages::too_large(l, &book.title, book.bytes, limit),
        );
    }
    if !kavita
        .email_setup(key)
        .await
        .map_err(|e| fault(metrics, who, &e))?
    {
        metrics.inc(KAVITA_FAILURES, "email_off");
        return Err(messages::email_off(l, admin));
    }
    match ask(messages::confirm(
        l,
        &book.title,
        book.format.label(),
        book.bytes,
        &device_name,
    ))
    .await
    {
        Answer::Accepted if !cancelled() => {}
        // Declined, no answer in time, or an accept that arrived after the call was abandoned.
        _ => return skipped(metrics, "declined", messages::not_sent(l)),
    }
    kavita
        .send_to(key, device.id, book.chapter_id)
        .await
        .map_err(|e| match fault(metrics, who, &e) {
            _ if matches!(e, KavitaError::Http5xx | KavitaError::Refused(_)) => {
                messages::send_refused(l, admin)
            }
            text => text,
        })?;
    tracing::info!(profile = %who.profile, chapter = book.chapter_id, "sent to Kindle");
    Ok(messages::sent(l))
}
