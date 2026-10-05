//! ODRS reviews (odrs.gnome.org): the review texts, where `ratings` only
//! fetches aggregated ratings. Same endpoint as GNOME Software
//! (`lib/gs-odrs-provider.c:889-915`): `POST /1.0/reviews/api/fetch` with a
//! `{user_hash, app_id, locale, distro, version, limit}` body.
//!
//! Reading is cosmetic: on network failure an empty list is returned and the
//! "Ratings & Reviews" section stays hidden. Writing goes to
//! `POST /1.0/reviews/api/submit`, signed with the `user_skey` ODRS returns
//! with every fetch for the user's hash.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const ENDPOINT: &str = "https://odrs.gnome.org/1.0/reviews/api/fetch";
const SUBMIT: &str = "https://odrs.gnome.org/1.0/reviews/api/submit";
const LIMIT: u32 = 10;

/// Hash sent when only reading: shared by every Onyx user, so reading tells
/// ODRS nothing about who looks at which app.
const ANONYMOUS_HASH: &str = "0000000000000000000000000000000000000000";

/// The user's ODRS id for posting: 40 random hex digits made once and kept in
/// the settings. `None` when no randomness is available: never a guessable id.
fn author_hash() -> Option<String> {
    let mut settings = crate::settings::load_settings();
    if let Some(h) = &settings.odrs_user_hash {
        return Some(h.clone());
    }
    let mut bytes = [0u8; 20];
    std::io::Read::read_exact(&mut std::fs::File::open("/dev/urandom").ok()?, &mut bytes).ok()?;
    let hash: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    settings.odrs_user_hash = Some(hash.clone());
    crate::settings::save_settings(&settings).ok()?;
    Some(hash)
}

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

/// An app's reviews, cached for the session (like metadata). ODRS ranks
/// reviews of the same version first.
pub fn reviews_for(app_id: &str, version: &str) -> Vec<Review> {
    if app_id.is_empty() {
        return Vec::new();
    }
    if let Some(hit) = CACHE.lock().ok().and_then(|c| c.get(app_id).cloned()) {
        return hit;
    }
    let fetched = fetch_reviews(app_id, version).unwrap_or_default();
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

fn fetch_reviews(app_id: &str, version: &str) -> Option<Vec<Review>> {
    let body = serde_json::json!({
        "user_hash": ANONYMOUS_HASH,
        "app_id": app_id,
        "locale": locale(),
        "distro": "NixOS",
        "version": odrs_version(version),
        "limit": LIMIT,
    });
    crate::http::post_json(ENDPOINT, &body).map(|v| parse_reviews(&v))
}

/// ODRS rejects an empty version ("missing data, expected version").
fn odrs_version(version: &str) -> &str {
    if version.is_empty() {
        "unknown"
    } else {
        version
    }
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

/// Sends a review of `app_id` to ODRS. `stars` from 1 to 5. Blocking.
pub fn submit_review(
    app_id: &str,
    version: &str,
    stars: u8,
    summary: &str,
    description: &str,
    author: &str,
) -> Result<(), String> {
    let hash = author_hash().ok_or_else(|| {
        crate::util::n_("No random source to create your reviewer id.").to_string()
    })?;
    let fetched = crate::http::post_json(
        ENDPOINT,
        &serde_json::json!({
            "user_hash": hash, "app_id": app_id, "locale": locale(),
            "distro": "NixOS", "version": odrs_version(version), "limit": 1,
        }),
    )
    .ok_or_else(|| crate::util::n_("ODRS cannot be reached.").to_string())?;
    let skey = fetched
        .as_array()
        .and_then(|a| a.iter().find_map(|i| i["user_skey"].as_str()))
        .ok_or_else(|| crate::util::n_("ODRS did not accept this app.").to_string())?;
    let body = serde_json::json!({
        "user_hash": hash,
        "user_skey": skey,
        "app_id": app_id,
        "locale": locale(),
        "distro": "NixOS",
        "version": odrs_version(version),
        "summary": summary,
        "description": description,
        "user_display": author,
        "rating": u32::from(stars.clamp(1, 5)) * 20,
    });
    let res = crate::http::post_json(SUBMIT, &body)
        .ok_or_else(|| crate::util::n_("ODRS cannot be reached.").to_string())?;
    if res["success"].as_bool() == Some(true) {
        if let Ok(mut c) = CACHE.lock() {
            c.remove(app_id);
        }
        Ok(())
    } else {
        Err(res["msg"]
            .as_str()
            .unwrap_or("ODRS refused the review.")
            .to_string())
    }
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
