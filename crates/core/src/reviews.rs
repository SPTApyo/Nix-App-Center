//! Avis ODRS (odrs.gnome.org) — le texte des avis, là où `ratings` ne récupère
//! que les notes agrégées. Même endpoint que GNOME Software
//! (`lib/gs-odrs-provider.c:889-915`) : `POST /1.0/reviews/api/fetch` avec un
//! corps `{user_hash, app_id, locale, distro, version, limit}`.
//!
//! Purement cosmétique : en cas d'échec réseau on rend une liste vide et la
//! section « Évaluations » reste masquée.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

const ENDPOINT: &str = "https://odrs.gnome.org/1.0/reviews/api/fetch";
const LIMIT: u32 = 10;

/// GNOME dérive un SHA-1 salé de l'utilisateur pour pouvoir rattacher ses propres
/// avis. Onyx ne fait que lire : un hash constant suffit, et n'identifie personne.
const USER_HASH: &str = "0000000000000000000000000000000000000000";

/// Un avis ODRS. `stars` est déjà converti depuis le `rating` 0–100 de l'API.
#[derive(Debug, Clone)]
pub struct Review {
    pub summary: String,
    pub description: String,
    pub author: Option<String>,
    pub stars: f32,
    pub timestamp: i64,
    /// Pertinence calculée par ODRS (karma, ancienneté…) — sert à élire l'avis vedette.
    pub score: i64,
}

static CACHE: LazyLock<Mutex<HashMap<String, Vec<Review>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Avis d'une app, mis en cache pour la session (comme les métadonnées).
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

/// L'avis vedette = le mieux classé par ODRS, comme `get_featured_review`
/// (gs-details-page.c). Fonction pure, testée.
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

/// Locale au format ODRS (`fr_FR`), depuis l'environnement. `C` par défaut.
fn locale() -> String {
    std::env::var("LC_MESSAGES")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .map(|l| l.split('.').next().unwrap_or("C").to_string())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "C".to_string())
}

/// Parse la réponse ODRS (une erreur `{…}` au lieu d'une liste → vide).
/// Séparé de l'appel réseau pour être testable.
fn parse_reviews(v: &serde_json::Value) -> Vec<Review> {
    let Some(items) = v.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let str_of = |k: &str| item.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
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
                // ODRS note sur 100 (20/40/60/80/100) ; AppStream aussi. → /20.
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
        {"app_id":"org.gnome.gedit","summary":"Correct","description":"Fait le travail.",
         "user_display":"Alice","rating":60,"date_created":1600000000,"score":3,
         "karma_up":1,"karma_down":0},
        {"app_id":"org.gnome.gedit","summary":"Excellent","description":"Rien à redire.",
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
        assert_eq!(r.len(), 2, "l'avis sans description est écarté");
        assert_eq!(r[0].stars, 3.0);
        assert_eq!(r[1].stars, 5.0);
        assert_eq!(r[1].author.as_deref(), Some("Bob"));
        assert_eq!(r[1].timestamp, 1_700_000_000);
    }

    #[test]
    fn featured_is_highest_score() {
        let r = parse(FIXTURE);
        // Carol a le meilleur score (40) mais pas de description → écartée au parse.
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
