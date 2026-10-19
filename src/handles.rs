//! Short handles for search results, per person. send_to_kindle accepts only a handle this person's own
//! search minted, so the model can never name a chapter the person cannot see.
//! In memory only: a restart costs a re-search.

use std::collections::hash_map::RandomState;
use std::collections::{HashMap, VecDeque};
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::kavita::Format;

pub const TTL: Duration = Duration::from_secs(60 * 60);
pub const PER_PERSON: usize = 50;
const LEN: usize = 6;

#[derive(Debug, Clone)]
pub struct Book {
    pub chapter_id: u32,
    pub title: String,
    pub format: Format,
    pub bytes: u64,
    pub library: String,
}

struct Entry {
    handle: String,
    book: Book,
    minted: Instant,
}

#[derive(Default)]
pub struct Handles {
    by_person: Mutex<HashMap<String, VecDeque<Entry>>>,
    seed: RandomState,
    counter: AtomicU64,
}

impl Handles {
    /// Letters only, so a bare chapter number never equals a handle. Not a secret: another person's
    /// handle is refused whatever its value, so uniqueness per person is all that matters.
    fn fresh(&self, taken: &VecDeque<Entry>) -> String {
        loop {
            let mut n = self
                .seed
                .hash_one(self.counter.fetch_add(1, Ordering::Relaxed));
            let h: String = (0..LEN)
                .map(|_| {
                    let c = b'a' + (n % 26) as u8;
                    n /= 26;
                    c as char
                })
                .collect();
            if !taken.iter().any(|e| e.handle == h) {
                return h;
            }
        }
    }

    pub fn mint(&self, profile: &str, book: Book, now: Instant) -> String {
        let mut map = self.by_person.lock().expect("handle lock");
        let q = map.entry(profile.to_string()).or_default();
        q.retain(|e| now.duration_since(e.minted) < TTL);
        while q.len() >= PER_PERSON {
            q.pop_front();
        }
        let handle = self.fresh(q);
        q.push_back(Entry {
            handle: handle.clone(),
            book,
            minted: now,
        });
        handle
    }

    #[must_use]
    pub fn resolve(&self, profile: &str, handle: &str, now: Instant) -> Option<Book> {
        let map = self.by_person.lock().expect("handle lock");
        map.get(profile)?
            .iter()
            .find(|e| e.handle == handle && now.duration_since(e.minted) < TTL)
            .map(|e| e.book.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kavita::Format;

    fn book(id: u32) -> Book {
        Book {
            chapter_id: id,
            title: format!("t{id}"),
            format: Format::Epub,
            bytes: 1,
            library: "Fiction".into(),
        }
    }

    #[test]
    fn a_handle_is_six_letters_and_resolves_for_its_owner() {
        let (h, now) = (Handles::default(), Instant::now());
        let x = h.mint("alice", book(10), now);
        assert_eq!(x.len(), 6);
        assert!(x.bytes().all(|b| b.is_ascii_lowercase()), "{x}");
        assert_eq!(h.resolve("alice", &x, now).unwrap().chapter_id, 10);
    }

    #[test]
    fn another_persons_handle_is_refused() {
        let (h, now) = (Handles::default(), Instant::now());
        let x = h.mint("alice", book(10), now);
        assert!(h.resolve("bob", &x, now).is_none());
    }

    #[test]
    fn a_bare_chapter_id_or_a_made_up_handle_is_refused() {
        let (h, now) = (Handles::default(), Instant::now());
        h.mint("alice", book(10), now);
        for guess in ["10", "123456", "abcdef", ""] {
            assert!(h.resolve("alice", guess, now).is_none(), "{guess}");
        }
    }

    #[test]
    fn a_handle_expires_after_sixty_minutes() {
        let (h, now) = (Handles::default(), Instant::now());
        let x = h.mint("alice", book(10), now);
        assert!(h
            .resolve("alice", &x, now + TTL - Duration::from_secs(1))
            .is_some());
        assert!(h.resolve("alice", &x, now + TTL).is_none());
    }

    #[test]
    fn the_fifty_first_handle_evicts_the_oldest() {
        let (h, now) = (Handles::default(), Instant::now());
        let first = h.mint("alice", book(0), now);
        let rest: Vec<_> = (1..=PER_PERSON as u32)
            .map(|i| h.mint("alice", book(i), now))
            .collect();
        assert!(h.resolve("alice", &first, now).is_none());
        assert!(h.resolve("alice", &rest[0], now).is_some());
        assert!(h.resolve("alice", rest.last().unwrap(), now).is_some());
    }

    #[test]
    fn handles_are_distinct_within_a_person() {
        let (h, now) = (Handles::default(), Instant::now());
        let all: std::collections::HashSet<_> = (0..PER_PERSON as u32)
            .map(|i| h.mint("j", book(i), now))
            .collect();
        assert_eq!(all.len(), PER_PERSON);
    }
}
