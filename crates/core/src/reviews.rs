//! ODRS reviews (odrs.gnome.org): the review texts, where `ratings` only
//! fetches aggregated ratings. Same endpoint as GNOME Software
//! (`lib/gs-odrs-provider.c:889-915`): `POST /1.0/reviews/api/fetch` with a
//! `{user_hash, app_id, locale, distro, version, limit}` body.
//!
//! Purely cosmetic: on network failure an empty list is returned and the
//! "Ratings & Reviews" section stays hidden.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const ENDPOINT: &str = "https://odrs.gnome.org/1.0/reviews/api/fetch";
const LIMIT: u32 = 10;

/// GNOME derives a salted SHA-1 of the user to link their own reviews. Onyx
/// only reads: a constant hash is enough and identifies nobody.
const USER_HASH: &str = "0000000000000000000000000000000000000000";

/// An ODRS review. `stars` is already converted from the API's 0–100 `rating`.
#[derive(Debug, Clone)]
pub struct Review {
    pub summary: String,
    pub description: String,
    pub author: Option<String>,
    pub stars: f32,
    pub timestamp: i64,
    /// Relevance computed by ODRS (karma, age…), used to pick the featured review.
    pub score: i64,
}

static CACHE: LazyLock<Mutex<HashMap<String, Vec<Review>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// An app's reviews, cached for the session (like metadata).
pub fn reviews_for(app_id: &str) -> Vec<Review> {
    if app_id.is_empty() {
        return Vec::new();
    }
    if let Some(hit) = CACHE.lock().ok().and_then(|c| c.get(app_id).cloned()) {
        return hit;
    }
    let fetched = fetch_reviews(app_id).unwrap_or_default();
    if let Ok(mut c) = CACHE.lock() {
        c.insert(app_id.to_string(), fetched.clone());
    }
    fetched
}

/// The featured review = the best ranked by ODRS, like `get_featured_review`
/// (gs-details-page.c). Pure function, tested.
pub fn featured_review(reviews: &[Review]) -> Option<&Review> {
    reviews.iter().max_by_key(|r| r.score)
}

fn fetch_reviews(app_id: &str) -> Option<Vec<Review>> {
    let body = serde_json::json!({
        "user_hash": USER_HASH,
        "app_id": app_id,
        "locale": locale(),
        "distro": "NixOS",
        "version": "",
        "limit": LIMIT,
    });
    crate::http::post_json(ENDPOINT, &body).map(|v| parse_reviews(&v))
}

/// Locale in ODRS format (`fr_FR`), from the environment. `C` by default.
fn locale() -> String {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .map(|l| l.split('.').next().unwrap_or("C").to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "C".to_string())
}

/// Parses the ODRS response (an error `{…}` instead of a list gives empty).
/// Kept apart from the network call to be testable.
fn parse_reviews(v: &serde_json::Value) -> Vec<Review> {
    let Some(items) = v.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let str_of = |k: &str| {
                item.get(k)
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let int_of = |k: &str| item.get(k).and_then(|x| x.as_i64()).unwrap_or(0);

            let description = str_of("description");
            if description.trim().is_empty() {
                return None;
            }
            let author = Some(str_of("user_display")).filter(|s| !s.trim().is_empty());
            Some(Review {
                summary: str_of("summary"),
                description,
                author,
                // ODRS rates out of 100 (20/40/60/80/100), like AppStream: /20.
                stars: int_of("rating") as f32 / 20.0,
                timestamp: int_of("date_created"),
                score: int_of("score"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"[
        {"app_id":"org.gnome.gedit","summary":"Decent","description":"Does the job.",
         "user_display":"Alice","rating":60,"date_created":1600000000,"score":3,
         "karma_up":1,"karma_down":0},
        {"app_id":"org.gnome.gedit","summary":"Excellent","description":"Nothing to add.",
         "user_display":"Bob","rating":100,"date_created":1700000000,"score":12,
         "karma_up":9,"karma_down":0},
        {"app_id":"org.gnome.gedit","summary":"Vide","description":"   ",
         "user_display":"Carol","rating":20,"date_created":1500000000,"score":40}
    ]"#;

    fn parse(json: &str) -> Vec<Review> {
        parse_reviews(&serde_json::from_str(json).unwrap())
    }

    #[test]
    fn parse_converts_rating_to_stars() {
        let r = parse(FIXTURE);
        assert_eq!(r.len(), 2, "the review without description is dropped");
        assert_eq!(r[0].stars, 3.0);
        assert_eq!(r[1].stars, 5.0);
        assert_eq!(r[1].author.as_deref(), Some("Bob"));
        assert_eq!(r[1].timestamp, 1_700_000_000);
    }

    #[test]
    fn featured_is_highest_score() {
        let r = parse(FIXTURE);
        // Carol has the best score (40) but no description, dropped at parse time.
        assert_eq!(featured_review(&r).unwrap().summary, "Excellent");
    }

    #[test]
    fn parse_tolerates_garbage() {
        assert!(parse("{\"error\": \"nope\"}").is_empty());
        assert!(parse("[{}]").is_empty());
    }

    #[test]
    fn featured_of_empty_is_none() {
        assert!(featured_review(&[]).is_none());
    }
}
